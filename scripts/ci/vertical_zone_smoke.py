"""Run vertical-zone entry, navigation, privacy and playback in a real WebView."""

import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime

MENU = "/api/secondary/tab/list"
HOST_LIST = "/api/user/playlist"
PRIVACY = "/api/user/setting"
TAB = "/api/zone/tab/get"
CLASSIC = "/api/zone/get"
ALBUM = {"/api/album/v3/detail", "/api/album/v4/detail"}
PLAYLIST = "/api/v6/playlist/detail"
PLAYLIST_V4 = "/api/playlist/v4/detail"
PLAYLIST_DYNAMIC = "/api/playlist/detail/dynamic"
SONG = "/api/v3/song/detail"
PLAY = "/api/song/enhance/player/url/v1"
MODULES = ["carousel", "horizontal", "ranking", "grid"]
CASES = {
    "full": ("menu_open", "menu", "rock_modules", "album_back", "playlist_back", "playlist_play", "rock_hot", "jazz_fallback"),
    "privacy": ("privacy_gate", "privacy_settings"),
    "hotspot": ("hotspot_zone", "hotspot_back"),
    "playlist-detail": ("normal_detail", "normal_header", "normal_back", "mgc_detail", "mgc_header",
                        "mgc_row", "mgc_double", "mgc_back", "normal_reopen", "normal_replay"),
    "playlist-v6": ("ordinary_detail", "ordinary_play", "preflight_detail", "preflight_play",
                    "promoted_detail", "promoted_play", "liked_detail", "liked_play"),
    "playlist-batch": ("ordinary_play", "zone_detail", "batch_hydrated", "batch_play", "batch_append",
                       "batch_download", "batch_queued_play"),
    "playlist-queue-menu": (
        "zone_empty_detail", "zone_empty_next", "ordinary_detail", "ordinary_added",
        "zone_filtered_detail", "whole_added", "row_moved", "row_playing",
    ),
}
CASES["playlist-detail-focus"] = (
    "normal_detail", "normal_header", "normal_back", "mgc_detail", "mgc_header", "mgc_row",
    "row_current_paused", "row_current_resumed", "row_focus_yielded", "mgc_double", "mgc_back",
    "normal_reopen", "normal_replay", "row_pending_left",
)
DEFAULT_CASES = ("full", "privacy", "hotspot")


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def check_playlist_detail(rows):
    ids = ["81", "84", "87"]
    scopes = {
        "normal_header": ("81", -1), "mgc_header": ("81", 121),
        "mgc_row": ("87", 121), "mgc_double": ("84", 121), "normal_replay": ("81", -1),
    }
    relevant = {PLAYLIST, PLAYLIST_V4, PLAYLIST_DYNAMIC, SONG, PLAY}
    previous = []
    play_ids = set()
    for name, row in rows.items():
        wire = row["verticalZoneCalls"]
        assert wire[:len(previous)] == previous, name
        delta = [(call["path"], call["args"]) for call in wire[len(previous):] if call["path"] in relevant]
        previous = wire
        paths = [path for path, _ in delta]
        if name in {"normal_detail", "mgc_detail", "normal_reopen"}:
            mgc = name == "mgc_detail"
            assert row["personalized"] == mgc
            assert row["description"] == ("合成个性化歌单详情" if mgc else "合成普通歌单详情")
            assert row["subscribers"] == ("135" if mgc else None)
            if mgc:
                assert paths == [PLAYLIST_V4, PLAYLIST_DYNAMIC, SONG], delta
                assert delta[0][1] == {"id": "204", "n": 0, "s": 0}
                assert delta[1][1] == {"id": "204"}
            else:
                assert paths == [PLAYLIST, SONG], delta
                assert delta[0][1] == {"id": "204", "n": 100000, "s": 0}
            for path, args in delta:
                if path == SONG:
                    assert json.loads(args["c"]) == [{"id": value, "v": 0} for value in ids], delta
                    assert args.get("trialMode") == (121 if mgc else None), delta
        elif name in scopes:
            selected, trial = scopes[name]
            assert row["personalized"] == (trial == 121)
            assert row["queue"] == ids and row["trackId"] == selected and row["position"] > 0.15
            player = row["nativePlayer"]
            assert player["state"] == "playing" and not player["error"] and player["position"] > 0.15
            assert player["playId"] == row["playId"] and player["playId"] not in play_ids
            assert player["track"]["id"] == selected and player["origin"]["id"] == "204"
            assert row["audioFactoryCount"] == 1
            assert row["focusRetained"], name
            play_ids.add(player["playId"])
            assert PLAYLIST_V4 not in paths and PLAYLIST_DYNAMIC not in paths, delta
            if trial == 121:
                assert PLAYLIST not in paths and SONG in paths and PLAY in paths, delta
                assert paths.index(SONG) < paths.index(PLAY), delta
            hydrated = []
            authorized = []
            for path, args in delta:
                if path == SONG:
                    assert args.get("trialMode") == (121 if trial == 121 else None), delta
                    hydrated.extend(str(item["id"]) for item in json.loads(args["c"]))
                elif path == PLAY:
                    assert args["trialMode"] == trial, delta
                    packed = json.loads(args["ids"])
                    assert len(packed) == 1, delta
                    authorized.append(str(packed[0]))
            assert selected in authorized, (name, delta)
            if trial == 121:
                assert selected in hydrated, (name, delta)
        else:
            label = "专区普通同号歌单" if name == "normal_back" else "专区合成歌单 204"
            assert row["focus"] == label
            assert all(abs(row["expected"][axis] - row["actual"][axis]) <= 2 for axis in ("top", "left"))
    assert len(play_ids) == len(scopes)


def check_playlist_v6(rows):
    specs = {
        "ordinary": ("204", "合成普通同号详情", False, "81", None),
        "preflight": ("204", "合成卡片直达 V6", True, "81", 121),
        "promoted": ("205", "合成发现特殊类型后 V6", True, "81", 121),
        "liked": ("206", "合成本人红心 V6", True, "87", 43),
    }
    ids = ["81", "84", "87"]
    relevant = {PLAYLIST, PLAYLIST_V4, PLAYLIST_DYNAMIC, SONG, PLAY}
    previous = []
    play_ids = set()
    for stage, row in rows.items():
        name, step = stage.split("_")
        id_, description, personalized, selected, trial = specs[name]
        assert row["id"] == id_ and row["personalized"] == personalized, row
        wire = row["verticalZoneCalls"]
        assert wire[:len(previous)] == previous, stage
        delta = [(call["path"], call["args"]) for call in wire[len(previous):] if call["path"] in relevant]
        previous = wire
        paths = [path for path, _ in delta]
        if step == "detail":
            assert row["description"] == description, row
            assert row["subscribers"] == "7", (stage, "final detail subscriber count", row)
            detail = {"id": id_, "n": 100000, "s": 0} if trial is None else {
                "id": id_, "n": 500, "s": 0, "newStyle": True, "trialMode": trial,
            }
            expected = [(PLAYLIST, detail)]
            if name == "promoted":
                expected[:0] = [(PLAYLIST_V4, {"id": id_, "n": 0, "s": 0}),
                                (PLAYLIST_DYNAMIC, {"id": id_})]
            assert delta[:-1] == expected and paths[-1:] == [SONG], (stage, delta)
            assert delta[-1][1].get("trialMode") == trial, (stage, delta)
            assert json.loads(delta[-1][1]["c"]) == [{"id": value, "v": 0} for value in ids], (stage, delta)
            if name == "liked":
                assert any(call["path"] == HOST_LIST and call["args"]["uid"] == "2" for call in wire), wire
            continue
        assert row["queue"] == ids and row["trackId"] == selected and row["position"] > 0.15, row
        player = row["nativePlayer"]
        assert player["state"] == "playing" and not player["error"] and player["position"] > 0.15, player
        assert player["track"]["id"] == selected and player["origin"]["id"] == id_, player
        assert player["playId"] == row["playId"] and player["playId"] not in play_ids, player
        assert row["audioFactoryCount"] == 1 and row["focusRetained"], row
        play_ids.add(player["playId"])
        allowed = {SONG, PLAY} if personalized else {PLAYLIST, SONG, PLAY}
        assert all(path in allowed for path in paths), (stage, delta)
        assert SONG in paths and PLAY in paths and paths.index(SONG) < paths.index(PLAY), (stage, delta)
        for path, args in delta:
            if path == SONG:
                assert args.get("trialMode") == trial, (stage, delta)
            elif path == PLAY:
                assert args["trialMode"] == (trial if trial is not None else -1), (stage, delta)
        authorized = [str(value) for path, args in delta if path == PLAY for value in json.loads(args["ids"])]
        assert selected in authorized, (stage, delta)
    assert len(play_ids) == len(specs)


def check_playlist_batch(rows):
    ordinary, detail, hydrated, play, append, download, queued = rows.values()
    assert ordinary["queue"] == ["92", "93"] and ordinary["nativePlayer"]["track"]["id"] == "92"
    assert detail["personalized"] and len(hydrated["names"]) == 3 and "87" in hydrated["names"][2]
    assert play["queue"] == ["92", "87", "93"] and play["nativePlayer"]["track"]["id"] == "87"
    assert append["queue"] == ["92", "87", "84", "93"] and append["playId"] == play["playId"]
    assert download["title"] == "下载 1 首歌曲" and "84" in download["selection"]
    assert queued["nativePlayer"]["track"]["id"] == "84" and queued["playId"] != append["playId"]
    for snapshot in (ordinary, play, append, queued):
        assert snapshot["nativePlayer"]["state"] == "playing"
        assert snapshot["nativePlayer"]["playId"] == snapshot["playId"]
        assert snapshot["nativePlayer"]["position"] > 0.15
    calls = queued["verticalZoneCalls"]
    relevant = {PLAYLIST, PLAYLIST_V4, PLAYLIST_DYNAMIC, SONG}
    hydrated_delta = [call for call in hydrated["verticalZoneCalls"][len(detail["verticalZoneCalls"]):]
                      if call["path"] in relevant]
    assert len(hydrated_delta) == 1 and hydrated_delta[0]["path"] == SONG, hydrated_delta
    args = hydrated_delta[0]["args"]
    assert args["trialMode"] == -1 and json.loads(args["c"]) == [{"id": "87", "v": 0}], args
    after_detail = calls[len(detail["verticalZoneCalls"]):]
    assert all(call["path"] not in {PLAYLIST, PLAYLIST_V4, PLAYLIST_DYNAMIC} for call in after_detail)
    auth = {str(json.loads(call["args"]["ids"])[0]): call["args"]["trialMode"]
            for call in calls if call["path"] == PLAY}
    assert auth["92"] == -1 and auth["87"] == auth["84"] == 121, auth


def check_playlist_queue_menu(rows):
    previous = []
    relevant = {PLAYLIST, PLAYLIST_V4, PLAYLIST_DYNAMIC, SONG, PLAY}
    paths_by_stage = {
        "zone_empty_detail": [PLAYLIST_V4, PLAYLIST_DYNAMIC, SONG],
        "zone_empty_next": [PLAY],
        "ordinary_detail": [PLAYLIST, SONG],
        "ordinary_added": [PLAYLIST, SONG, PLAY],
        "zone_filtered_detail": [PLAYLIST_V4, PLAYLIST_DYNAMIC, SONG],
        "whole_added": [],
        "row_moved": [],
        "row_playing": [PLAY],
    }
    moved_authorizations = []
    for stage, row in rows.items():
        wire = row["verticalZoneCalls"]
        assert wire[:len(previous)] == previous, stage
        delta = [(call["path"], call["args"]) for call in wire[len(previous):] if call["path"] in relevant]
        previous = wire
        # Source prefetch can add a second authorized row on the same stage
        # or later. Treat only distinct operation reads as mandatory below.
        assert [path for path, _ in delta if path != PLAY] == [
            path for path in paths_by_stage[stage] if path != PLAY], (stage, delta)
        if stage in {"zone_empty_detail", "zone_filtered_detail"}:
            assert delta[-1][1]["trialMode"] == 121
        if stage == "ordinary_detail":
            assert "trialMode" not in delta[-1][1]
        if stage in {"row_moved", "row_playing"}:
            moved_authorizations.extend(args for path, args in delta if path == PLAY and
                                        list(map(str, json.loads(args["ids"]))) == ["87"])
        if stage in {"zone_empty_next", "ordinary_added", "row_playing"}:
            song, trial = ("92", -1) if stage == "ordinary_added" else ("87", 121)
            if stage == "row_playing":
                assert moved_authorizations and all(args["trialMode"] == trial for args in moved_authorizations), moved_authorizations
            else:
                assert any(path == PLAY and args["trialMode"] == trial and
                           list(map(str, json.loads(args["ids"]))) == [song] for path, args in delta), (stage, delta)
            player = row["nativePlayer"]
            assert player["state"] == "playing" and player["position"] > .15
            assert player["track"]["id"] == song and player["playId"] == row["playId"]
    assert rows["zone_empty_next"]["queue"] == ["87"]
    assert rows["ordinary_added"]["queue"] == ["92", "93"]
    assert rows["whole_added"]["queue"] == ["92", "81", "84", "87", "93"]
    assert rows["row_moved"]["queue"] == ["92", "87", "81", "84", "93"]
    assert rows["whole_added"]["playId"] == rows["row_moved"]["playId"] == rows["ordinary_added"]["playId"]
    assert rows["row_playing"]["playId"] != rows["row_moved"]["playId"]


def check_case(directory, case, screenshots, windows):
    output = directory / "first"
    complete = read(output / "complete.json")
    assert complete["ok"] and complete["stageCount"] == len(CASES[case])
    calls = complete["verticalZoneCalls"]
    zone_calls = [(call["path"], call["args"]) for call in calls if call["path"] in {TAB, CLASSIC}]
    expected = [(TAB, {"pageCode": "ROCK", "tabKey": "default"})]
    if case == "hotspot":
        expected.append((TAB, {"pageCode": "ROCK", "tabKey": "1"}))
    if case == "full":
        expected.extend([
            (TAB, {"pageCode": "ROCK", "tabKey": "1"}),
            (TAB, {"pageCode": "JAZZ", "tabKey": "default"}),
            (CLASSIC, {"pageCode": "JAZZ"}),
        ])
    assert zone_calls == expected, zone_calls
    menu_indices = [idx for idx, call in enumerate(calls) if call["path"] == MENU and call["args"] == {}]
    privacy_indices = [idx for idx, call in enumerate(calls) if call["path"] == PRIVACY and call["args"] == {}]
    first_zone = next(idx for idx, call in enumerate(calls) if call["path"] == TAB)
    assert menu_indices and privacy_indices and menu_indices[0] < first_zone and privacy_indices[0] < first_zone
    assert not any(call["path"] == PRIVACY + "/update" for call in calls), "privacy CTA wrote preferences"
    rows = {}
    for index, name in enumerate(CASES[case], start=1):
        path = output / f"ux_{index:02d}_{name}.json"
        row = rows[name] = read(path)
        assert row["width"] == 920 and row["height"] == 620, (name, row["width"], row["height"])
        if screenshots:
            assert path.with_suffix(".png").is_file()
    if case == "full":
        assert rows["menu_open"]["focus"] == "JAZZ" and rows["menu"]["focus"] == "更多"
        assert rows["rock_modules"]["modules"] == MODULES
        assert rows["rock_hot"]["modules"] == rows["jazz_fallback"]["modules"] == ["grid"]
        assert rows["jazz_fallback"]["zoneTitle"] == "爵士"
        assert rows["album_back"]["focus"] == "专区合成专辑"
        assert rows["playlist_back"]["focus"] == "专区合成歌单 204"
        for name in ("album_back", "playlist_back"):
            row = rows[name]
            assert row["expected"]["top"] >= 60 and row["expected"]["left"] >= 75
            assert all(abs(row["expected"][axis] - row["actual"][axis]) <= 2 for axis in ("top", "left"))
        assert any(call["path"] in ALBUM and str(call["args"]["id"]) == "7" for call in calls), calls
        assert any(call["path"] == PLAYLIST and str(call["args"]["id"]) == "204" for call in calls), calls
        authorizations = [call["args"] for call in calls if call["path"] == PLAY]
        # The current track is required. The native actor may also prefetch
        # the sole next track before the captured run stops.
        assert 1 <= len(authorizations) <= 2, authorizations
        for expected_id, authorization in zip(("81", "84"), authorizations):
            assert authorization.get("trialMode") == 121, authorizations
            assert list(map(str, json.loads(authorization["ids"]))) == [expected_id], authorizations
        playback = rows["playlist_play"]
        player = playback["nativePlayer"]
        assert playback["tracks"] == ["81", "84"] and player["track"]["id"] == "81"
        assert playback["playId"] == player["playId"] and player["position"] > 0.15
        assert player["state"] == "playing" and playback["audioFactoryCount"] == 1
    elif case == "privacy":
        assert rows["privacy_gate"]["modules"] == MODULES
        assert rows["privacy_settings"]["focus"] == "settings-privacy"
        assert not any(call["path"] == PLAY for call in calls)
    elif case == "hotspot":
        assert rows["hotspot_zone"]["modules"] == ["grid"] and rows["hotspot_zone"]["zoneTitle"] == "摇滚"
        assert rows["hotspot_back"]["focus"] == "查看专区"
        assert any(call["path"] == "/api/search/pc/complex/page/v3" and
                   call["args"]["keyword"] == "合成专区热点" for call in calls)
        assert not any(call["path"] == PLAY for call in calls)
    elif case == "playlist-v6":
        check_playlist_v6(rows)
    elif case == "playlist-batch":
        check_playlist_batch(rows)
    elif case == "playlist-queue-menu":
        check_playlist_queue_menu(rows)
    elif case == "playlist-detail-focus":
        check_playlist_detail({name: rows[name] for name in CASES["playlist-detail"]})
        for name, state in [("row_current_paused", "paused"), ("row_current_resumed", "playing")]:
            row = rows[name]
            assert row["nativePlayer"]["playId"] == row["expectedPlayId"] == rows["mgc_row"]["playId"]
            assert row["nativePlayer"]["state"] == state and row["focusRetained"]
            assert row["nativePlayer"]["track"]["id"] == "87"
            assert row["nativePlayer"]["entryId"] == rows["mgc_row"]["nativePlayer"]["entryId"]
            assert row["nativePlayer"]["queueRevision"] == rows["mgc_row"]["nativePlayer"]["queueRevision"]
        yielded = rows["row_focus_yielded"]
        assert yielded["userFocusRetained"]
        assert yielded["nativePlayer"]["state"] == "playing" and yielded["nativePlayer"]["track"]["id"] == "84"
        assert yielded["nativePlayer"]["playId"] == yielded["playId"] != rows["mgc_row"]["playId"]
        assert yielded["nativePlayer"]["position"] > .15
        left = rows["row_pending_left"]
        assert left["detached"] and left["focus"] == "专区普通同号歌单"
        holds = left["focusHolds"]
        assert [(hold["id"], hold["mode"]) for hold in holds] == [("87", "restore"), ("84", "other"), ("84", "leave")]
        assert all(hold["disabledObserved"] and hold["disabledBlurInjected"] and hold["trackId"] == hold["id"] for hold in holds)
        assert holds[-1]["detachedBeforeReply"]
        assert left["nativePlayer"]["state"] == "playing" and left["nativePlayer"]["track"]["id"] == "84"
        assert left["nativePlayer"]["playId"] == holds[-1]["playId"] and left["nativePlayer"]["position"] > .15
        assert left["audioFactoryCount"] == 1
    else:
        check_playlist_detail(rows)
    if not windows:
        locale = read(output / "locale.json")
        assert locale["numeric"] == "C"
        assert all(locale[key].casefold().replace("-", "") == "en_us.utf8" for key in ("messages", "characters"))
    return {"states": list(rows), "zoneReads": zone_calls}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    parser.add_argument("--case", choices=CASES)
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if sys.platform not in {"linux", "win32"} or windows != bool(args.runtime) or windows and args.screenshots:
        parser.error("Windows requires --runtime and Linux alone permits screenshots")
    binary = args.binary.resolve(strict=True)
    base_env = {key: value for key, value in os.environ.items()
                if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_"))
                and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    media = None
    if windows:
        stage, media = windows_runtime(binary, args.runtime)
        base_env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profiles = ("APPDATA", "LOCALAPPDATA", "TEMP", "TMP")
    else:
        base_env["LC_ALL"] = "en_US.UTF-8"
        profiles = ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR")
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    probe = Path(__file__).resolve().parents[2] / "desktop/examples/vertical_zone_ui_probe.js"
    script_source = probe.read_text(encoding="utf-8")
    result = {}
    selected_cases = (args.case,) if args.case else DEFAULT_CASES
    for case in selected_cases:
        directory = root / case
        directory.mkdir()
        script = directory / "probe.js"
        native_case = "playlist-detail" if case == "playlist-detail-focus" else case
        script.write_text(f"window.__verticalZoneCase={json.dumps(native_case)};\n"
                          + ("window.__verticalZoneRowFocusProof=true;\n" if case == "playlist-detail-focus" else "")
                          + script_source, encoding="utf-8")
        env = {**base_env, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_MOTION_PROOF": "1",
               "CUMULUS_VERTICAL_ZONE_PROOF": native_case, "CUMULUS_UX_SCRIPT": str(script)}
        for key in profiles:
            isolated = directory / "profile" / key
            isolated.mkdir(parents=True, mode=0o700)
            env[key] = str(isolated)
        if args.frontend_dist:
            env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
        output = directory / "first"
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            code = launch(binary, env, log, output, args.screenshots)
        failed = output / "failed.json"
        assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
        assert code == 0, (case, code, directory)
        result[case] = check_case(directory, case, args.screenshots, windows)
    states = sum(len(CASES[case]) for case in selected_cases)
    summary = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, windowsMediaRuntime=media,
                   stateCount=states, screenshots=states if args.screenshots else 0,
                   realMusicRequests=0, realAccountWrites=0, cases=result)
    (root / "audit.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
