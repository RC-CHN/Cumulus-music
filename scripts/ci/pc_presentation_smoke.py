"""Live global PC presentation and cached search via signed synthetic full AB reads.

Only the sealed host's server response changes. App subscription, native receipt
projection, route/Back restoration and libmpv/null output remain production code.
The refresh command is a native test trigger equivalent to the lifecycle worker;
it does not wait 30 minutes or create a renderer-facing production refresh API.
"""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch
from playback_power_smoke import windows_runtime
from search_footer_smoke import assignment_arguments


LABELS = ("initial", "paused", "current_control", "current_treatment", "temporary_failure",
          "detail", "detail_control", "back_pending", "back_control", "back_treatment")
ASSIGNMENT = "/api/rtrs/abt/front/expinfo/list"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def verify(root, repository, screenshots, windows):
    output = root / "first"
    failed = output / "failed.json"
    assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
    done = read(output / "complete.json")
    assert done["ok"] and done["pcPresentationVerified"] and done["stageCount"] == 10
    assert done["realMusicRequests"] == done["realAccountWrites"] == 0
    paths = sorted(output.glob("ux_*.json"))
    assert [path.name for path in paths] == [f"ux_{index:02}_{label}.json" for index, label in enumerate(LABELS, 1)]
    stages = dict(zip(LABELS, map(read, paths)))
    trace = done["signedSearch"]
    assert trace["transport"] == "MusicHttpClient" and trace["syntheticOnly"] and trace["sameProfileAccountRestore"]
    assert trace["footerCase"] == trace["serverMode"] == "t1" and trace["serverGeneration"] == 5
    calls = trace["calls"]
    assert [call["path"] for call in calls] == ["/api/w/nuser/account/get"] + [ASSIGNMENT] * 6, calls
    assert calls[0]["args"] == {} and calls[0]["responseCode"] == 200
    assert all(call["signedPcContextVerified"] and call["sameAccountCredential"] for call in calls)
    modes = ("t1", "c", "t1", "error", "c", "t1")
    for generation, (call, mode) in enumerate(zip(calls[1:], modes)):
        assert call["args"] == assignment_arguments(repository)
        assert call["serverMode"] == mode and call["serverGeneration"] == generation
        if mode == "error":
            assert call.get("response") == "synthetic_error" and "responseCode" not in call
        else:
            assert call["responseCode"] == 200
    queries = done["presentationQueryCalls"]
    assert queries == [
        {"path": "/api/search/pc/result/tab", "args": {"keyword": "合成 V3"}},
        {"path": "/api/search/pc/complex/page/v3", "args": {
            "keyword": "合成 V3", "scene": "normal", "needCorrect": True,
            "channel": "typing", "bizQueryInfo": ""}},
    ], queries
    commands = done["refreshCommands"]
    assert [item["mode"] for item in commands] == list(modes[1:])
    events = done["presentationEvents"]
    for index, item in enumerate(commands, 1):
        assert item["beforeFull"] == index and item["afterFull"] == index + 1
        before, after = item["before"], item["after"]
        if item["mode"] == "error":
            assert after == before, "failure retains prior view even if refresh returns Ok(lastview)"
        else:
            assert before["revision"] != after["revision"] and after in events
            assert all(after[key] == (item["mode"] == "t1")
                       for key in ("searchFooter", "searchExpanded", "searchDense"))
    projections = done["receiptProjections"]
    assert len(projections) == 4, projections
    assert all(item["receiptPresent"] and item["revision"] == item["replyRevision"]
               for item in projections), projections
    paused = stages["paused"]["nativePlayer"]
    queue = stages["paused"]["nativeQueue"]
    assert paused["state"] == "paused" and paused["position"] > 2
    assert [row["track"]["id"] for row in queue["tracks"]] == ["1"]
    controls = {"current_control", "back_control"}
    treatments = {"initial", "paused", "current_treatment", "temporary_failure", "back_treatment"}
    for label, stage in stages.items():
        assert stage["label"] == label and stage["noOverflow"] and stage["audioFactoryCount"] == 1
        assert stage["presentationQueryCalls"] == queries, "no search replay while assignment or Back changes"
        bar, state = stage["playerbar"], stage["nativePlayer"]
        assert bar["state"] == state["state"]
        if label != "initial":
            assert state["state"] == "paused" and state["playId"] == paused["playId"]
            assert state["entryId"] == paused["entryId"] and stage["nativeQueue"] == queue
            assert state["queueRevision"] == paused["queueRevision"]
            assert abs(state["position"] - paused["position"]) < .04
            assert bar["playId"] == state["playId"] and bar["trackId"] == "1" and bar["position"] > 2
            assert bar["title"] == state["track"]["name"]
        if label in controls | treatments:
            treatment = label in treatments
            assert stage["footerCount"] == (3 if treatment else 0)
            assert stage["renderedTracks"] == 3
            assert stage["bestKeys"] == [f"artist:{index}" for index in range(100, 103 if treatment else 102)]
            assert stage["regularKeys"] == [f"artist:{index}" for index in range(200, 210 if treatment else 206)]
            assert stage["denseGap"] == ("16px" if treatment else None)
        else:
            assert stage["footerCount"] == stage["renderedTracks"] == 0
    for label in ("paused", "current_control", "current_treatment", "temporary_failure", "back_control", "back_treatment"):
        assert abs(stages[label]["scrollTop"] - 70) < 1, (label, stages[label]["scrollTop"])
    for label in ("current_control", "back_control", "back_treatment"):
        assert stages[label]["focusCard"] == "song:3" and stages[label]["focusCover"]
    assert stages["current_treatment"]["focusSearch"] and stages["temporary_failure"]["focusSearch"]
    assert stages["detail"]["reasonTitle"] == stages["detail_control"]["reasonTitle"] == "合成推荐详情"
    assert stages["back_pending"]["heldProjection"] and stages["back_pending"]["renderedTracks"] == 0
    assert done["nativePlayer"]["playId"] == paused["playId"] and done["nativePlayer"]["state"] == "paused"
    assert done["nativeQueue"] == queue
    urls = [item["args"] for item in done["hotPlaybackCalls"] if item["path"] == "/api/song/enhance/player/url/v1"]
    assert urls and all([str(item) for item in json.loads(args["ids"])] == ["1"] for args in urls)
    locale = None
    if not windows:
        locale = read(output / "locale.json")
        assert locale["numeric"] == "C"
        assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                   for key in ("messages", "characters"))
    pictures = len(list(output.glob("ux_*.png")))
    assert pictures == (10 if screenshots else 0)
    return dict(ok=True, actualWebView=True, genuineSignedPcTransport=True, sameCredentialRestore=True,
                stageCount=10, screenshots=pictures, fullAssignmentReads=6, comprehensiveSearchReads=2,
                nativeReceiptProjections=4, nativeNullPlaybackVerified=True, locale=locale,
                realMusicRequests=0, realAccountWrites=0,
                lifecycleBoundary="The sealed native refresh trigger exercises the same full signed read; startup/30-minute/account cancellation timing is covered by Rust unit tests, not by waiting here.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    args = parser.parse_args()
    windows = os.name == "nt"
    if windows and (not args.runtime or args.screenshots):
        parser.error("Windows requires --runtime and supports no GTK screenshot capture")
    if not windows and args.runtime:
        parser.error("--runtime identifies the Windows native media stage only")
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_"))
           and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    windows_media = None
    if windows:
        stage, windows_media = windows_runtime(binary, args.runtime)
        env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profiles = [("APPDATA", "roaming"), ("LOCALAPPDATA", "local"), ("TEMP", "temp")]
    else:
        env.update(LC_ALL="en_US.UTF-8", GDK_BACKEND="x11")
        profiles = [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                    ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")]
    repository = Path(__file__).resolve().parents[2]
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF="1", CUMULUS_SEARCH_UI_PROOF="1",
               CUMULUS_SEARCH_V3_PROOF="1", CUMULUS_SEARCH_HOT_PLAYBACK_PROOF="1",
               CUMULUS_SEARCH_FOOTER_UI_PROOF="1", CUMULUS_SEARCH_FOOTER_PROOF="t1",
               CUMULUS_PC_PRESENTATION_PROOF="1",
               CUMULUS_UX_SCRIPT=str(repository / "desktop/examples/pc_presentation_ui_probe.js"))
    for key, name in profiles:
        directory = root / "os-profile" / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if windows:
        env["TMP"] = env["TEMP"]
    if args.frontend_dist:
        env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
    with (root / "host.log").open("x", encoding="utf-8") as log:
        code = launch(binary, env, log, root / "first", args.screenshots)
    report = verify(root, repository, args.screenshots, windows)
    assert code == 0, (code, str(root / "host.log"))
    report["windowsMedia"] = windows_media
    (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
