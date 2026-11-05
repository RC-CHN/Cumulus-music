"""Four sealed signed-assignment search cohorts in a real WebView/null libmpv.

The failed-assignment case seeds historical V3 layout only. It must retain normal
search/playback while withholding footer hints and recommendation actions. No
real account, payment, service write, or audio output device is used.
"""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch
from playback_power_smoke import windows_runtime
from search_signed_trace import assignment_arguments, signed_assignment


CASES = ("t1", "c", "unknown", "error")
PLAYER_URL = "/api/song/enhance/player/url/v1"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def verify(directory, case, expected_assignment, screenshots, windows):
    output = directory / "first"
    failure = output / "failed.json"
    assert not failure.exists(), failure.read_text(encoding="utf-8") if failure.exists() else ""
    complete = read(output / "complete.json")
    assert all(complete[key] is True for key in ("ok", "searchFooterVerified", "nativeNullPlaybackVerified"))
    assert complete["footerCase"] == case and complete["historicalLayoutOnly"] == (case == "error")
    assert complete["realMusicRequests"] == complete["realAccountWrites"] == 0
    labels = ["results", "playing", "paused"] + (["recommendation", "back"] if case == "t1" else [])
    assert complete["stageCount"] == len(labels)
    paths = sorted(output.glob("ux_*.json"))
    assert [path.name for path in paths] == [f"ux_{index:02}_{label}.json" for index, label in enumerate(labels, 1)]
    states = dict(zip(labels, map(read, paths)))
    for label, state in states.items():
        signed_assignment(state["signedSearch"], case, expected_assignment)
        assert state["footerCase"] == case and state["label"] == label and state["noOverflow"]
        player = state["nativePlayer"]
        assert state["observedPlayId"] == player["playId"] and state["observedState"] == player["state"]
        bar = state["playerbar"]
        assert bar["state"] == player["state"], state
        if player.get("track"):
            assert bar["playId"] == player["playId"] and bar["trackId"] == player["track"]["id"]
            assert bar["title"] == player["track"]["name"] and bar["position"] > .2
        if label == "recommendation":
            assert state["reasonTitle"] == "合成推荐详情" and state["footerCount"] == 0
        else:
            assert state["query"] == "合成 V3" and state["category"] == "all"
            assert state["cardKeys"] == ["song:1", "song:2", "song:3"]
            assert state["footerCount"] == (3 if case == "t1" else 0)
        assert state["audioFactoryCount"] == 1, "all phases must use one actual native player"
    assert states["results"]["nativePlayer"]["state"] == "idle"
    playing = states["playing"]["nativePlayer"]
    paused = states["paused"]["nativePlayer"]
    assert playing["state"] == "playing" and playing["position"] > .25
    assert paused["state"] == "paused" and paused["position"] >= playing["position"] - .04
    queue = states["playing"]["nativeQueue"]
    assert [row["track"]["id"] for row in queue["tracks"]] == ["1"], "official single-card action queues only the selected song"
    assert queue["current"] == playing["entryId"] and queue["revision"] == playing["queueRevision"]
    for label in labels[1:]:
        state = states[label]
        player = state["nativePlayer"]
        assert player["resourceKind"] == "catalog" and player["track"]["id"] == "1"
        assert player["playId"] == playing["playId"] and player["entryId"] == playing["entryId"]
        assert player["queueRevision"] == playing["queueRevision"]
        assert state["nativeQueue"] == queue, "pause and reason navigation retain the exact native queue"
        if label != "playing":
            assert player["state"] == "paused" and abs(player["position"] - paused["position"]) < .04
    if case == "t1":
        assert states["back"]["focusAction"] == "reason:0"
    assert complete["nativePlayer"]["playId"] == paused["playId"]
    assert complete["nativePlayer"]["state"] == "paused" and complete["nativeQueue"] == queue
    signed_assignment(complete["signedSearch"], case, expected_assignment)
    media = complete["hotPlaybackCalls"]
    assert all(call["path"] in ("/api/v3/song/detail", PLAYER_URL) for call in media), media
    urls = [call["args"] for call in media if call["path"] == PLAYER_URL]
    assert urls and all([str(value) for value in json.loads(call["ids"])] == ["1"]
                        and call["level"] == "standard" for call in urls), urls
    assert complete["audioFactoryCount"] == 1 and complete["syntheticV3Calls"] >= 2
    locale = None
    if not windows:
        locale = read(output / "locale.json")
        assert locale["numeric"] == "C"
        assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                   for key in ("messages", "characters")), locale
    pictures = len(list(output.glob("ux_*.png")))
    assert pictures == (len(labels) if screenshots else 0)
    report = dict(ok=True, footerCase=case, actualWebView=True, realSignedEapiTransport=True,
                  sameCredentialAccountRestore=True, nativeNullPlaybackVerified=True,
                  stageCount=len(labels), screenshots=pictures, locale=locale,
                  footerCount=3 if case == "t1" else 0, historicalLayoutOnly=case == "error",
                  realMusicRequests=0, realAccountWrites=0)
    (directory / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    return report



def verify_vinyl_origin(directory, screenshots):
    output = directory / "first"
    labels = ["retained_playlist", "search_plain", "returned_search"]
    paths = sorted(output.glob("ux_*.json"))
    assert [path.stem for path in paths] == [f"ux_{n:02}_{label}" for n, label in enumerate(labels, 1)]
    retained, source, returned = map(read, paths)
    complete = read(output / "complete.json")
    assert complete["ok"] and complete["stageCount"] == len(labels)
    assert complete["realMusicRequests"] == complete["realAccountWrites"] == 0
    identity = lambda view: tuple(view[key] for key in ("playId", "entryId", "queueRevision"))
    for row in (retained, source, returned):
        native = row["nativePlayer"]
        assert native["state"] == "paused" and native["resourceKind"] == "catalog" and native["error"] is None
        assert identity(native) == identity(row["observed"]) and row["audioFactoryCount"] == 1
        assert row["nativeQueue"]["current"] == native["entryId"]
    playlist = retained["nativePlayer"]
    search = source["nativePlayer"]
    assert playlist["track"]["id"] == "1" and playlist["origin"] == {"kind": "playlist", "id": "7"}
    assert "searchOrigin" not in playlist and retained["sourceButton"] == "查看播放来源：歌单"
    assert retained["sourceText"] == "来源：歌单"
    assert search["track"]["id"] == "2" and search["searchOrigin"] is True and search["origin"] is None
    assert source["sourceText"] == "来源：搜索" and source["sourceButton"] is None
    assert returned["sameSearch"] and returned["sourceText"] is None and returned["sourceButton"] is None
    assert identity(search) == identity(returned["nativePlayer"]) == identity(complete["nativePlayer"])
    assert returned["nativePlayer"]["searchOrigin"] is True
    assert source["nativeQueue"] == returned["nativeQueue"] == complete["nativeQueue"]
    assert search["playId"] != playlist["playId"]
    assert source["syntheticV3Calls"] == returned["syntheticV3Calls"] == complete["syntheticV3Calls"]
    def selected_urls(row):
        urls = [call["args"] for call in row["hotPlaybackCalls"] if call["path"] == PLAYER_URL]
        result = []
        for call in urls:
            ids = [str(song) for song in json.loads(call["ids"])]
            if ids in (["1"], ["2"]):
                result.append((ids, call["trialMode"]))
        return result

    # The normal queue may independently prefetch song 3. Opening the lyric
    # details or clicking their text must not authorize either selected song.
    requested = selected_urls(complete)
    assert selected_urls(source) == selected_urls(returned) == requested
    assert requested.count((["1"], -1)) == 1 and requested.count((["2"], 42)) == 1, requested
    pictures = len(list(output.glob("ux_*.png")))
    assert pictures == (len(labels) if screenshots else 0)
    return dict(ok=True, actualWebView=True, nativeNullOutputOnly=True, states=labels, screenshots=pictures,
                sourceLabel="搜索", playlistLinkRetained=True, realMusicRequests=0, realAccountWrites=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path, help="exact staged Windows native media directory")
    parser.add_argument("--screenshots", action="store_true", help="optional isolated Linux GTK captures")
    parser.add_argument("--vinyl-origin", action="store_true", help="verify the original non-link search source")
    args = parser.parse_args()
    windows = os.name == "nt"
    if windows and (not args.runtime or args.screenshots):
        parser.error("Windows requires --runtime and does not support GTK screenshot capture")
    if not windows and args.runtime:
        parser.error("--runtime identifies the Windows media stage only")
    binary = args.binary.resolve(strict=True)
    base = {key: value for key, value in os.environ.items()
            if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_"))
            and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    windows_media = None
    if windows:
        stage, windows_media = windows_runtime(binary, args.runtime)
        base["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profile = [("APPDATA", "roaming"), ("LOCALAPPDATA", "local"), ("TEMP", "temp")]
    else:
        base.update(LC_ALL="en_US.UTF-8", GDK_BACKEND="x11")
        profile = [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                   ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")]
    repository = Path(__file__).resolve().parents[2]
    script_name = "search_vinyl_origin_ui_probe.js" if args.vinyl_origin else "search_footer_ui_probe.js"
    source = (repository / "desktop/examples" / script_name).read_text(encoding="utf-8")
    expected_assignment = assignment_arguments(repository)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    reports = []
    for case in (("t1",) if args.vinyl_origin else CASES):
        directory = root / case
        directory.mkdir()
        script = directory / "probe.js"
        script.write_text(f"window.__searchFooterCase={json.dumps(case)};\n" + source, encoding="utf-8")
        env = {**base, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_UX_SCRIPT": str(script),
               "CUMULUS_MOTION_PROOF": "1", "CUMULUS_SEARCH_UI_PROOF": "1",
               "CUMULUS_SEARCH_V3_PROOF": "1", "CUMULUS_SEARCH_HOT_PLAYBACK_PROOF": "1",
               "CUMULUS_SEARCH_FOOTER_PROOF": case, "CUMULUS_SEARCH_FOOTER_UI_PROOF": "1"}
        if args.vinyl_origin:
            env["CUMULUS_LIBRARY_UI_PROOF"] = "1"
        for key, name in profile:
            isolated = directory / "os-profile" / name
            isolated.mkdir(parents=True, mode=0o700)
            env[key] = str(isolated)
        if windows:
            env["TMP"] = env["TEMP"]
        if args.frontend_dist:
            env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            code = launch(binary, env, log, directory / "first", args.screenshots)
        failure = directory / "first/failed.json"
        assert not failure.exists(), failure.read_text(encoding="utf-8") if failure.exists() else ""
        assert code == 0, (case, code, str(directory / "host.log"))
        if args.vinyl_origin:
            report = verify_vinyl_origin(directory, args.screenshots)
            signed_assignment(read(directory / "first/complete.json")["signedSearch"], case, expected_assignment)
            (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
            print(json.dumps(report, ensure_ascii=False))
            return
        reports.append(verify(directory, case, expected_assignment, args.screenshots, windows))
    report = dict(ok=True, cases=reports, actualWebView=True, windowsMedia=windows_media,
                  nativeNullOutputOnly=True, historicalLayoutBoundary="error seeds only the prior V3 layout; failed live signed assignment cannot grant footers",
                  screenshots=sum(case["screenshots"] for case in reports),
                  realMusicRequests=0, realAccountWrites=0)
    (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
