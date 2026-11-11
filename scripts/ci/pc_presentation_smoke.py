"""Open history settings or cached search via signed synthetic full PC AB reads.

Only the sealed host's server response changes. App subscription, native receipt
projection, route/Back restoration and libmpv/null output remain production code.
The refresh command is a native test trigger equivalent to the lifecycle worker;
it does not wait 30 minutes or create a renderer-facing production refresh API.
"""
import argparse
import json
import os
import sqlite3
import struct
from pathlib import Path

from motion_smoke import launch
from playback_power_smoke import windows_runtime
from search_footer_smoke import assignment_arguments


LABELS = ("initial", "paused", "current_control", "current_treatment", "temporary_failure",
          "detail", "detail_control", "back_pending", "back_control", "back_treatment")
ASSIGNMENT = "/api/rtrs/abt/front/expinfo/list"
HISTORY_LABELS = (
    "untouched_treatment", "untouched_control", "untouched_treatment_again",
    "pending_during_experiment_off", "cancelled_without_choice",
    "confirmed_while_effectively_off", "manual_choice_retained",
)
PREVIEW_COHORTS = (
    ("c", False, False, None),
    ("density", False, True, "compact_grid"),
    ("expanded", True, False, "carousel"),
    ("both", True, True, "compact_carousel"),
)


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
    settlement = stages["paused"]["pauseSettlement"]
    first, observed = settlement["first"], settlement["observed"]
    assert settlement["quietMs"] >= 450 and int(observed["revision"]) - int(first["revision"]) >= 2
    clock_identity = ("epoch", "playId", "entryId", "queueRevision")
    assert all(first[key] == observed[key] == paused[key] for key in (*clock_identity, "state", "position"))
    assert all(not value["nativeSeeking"] and value["error"] is None for value in (first, observed, paused))
    assert [row["track"]["id"] for row in queue["tracks"]] == ["1"]
    controls = {"current_control", "back_control"}
    treatments = {"initial", "paused", "current_treatment", "temporary_failure", "back_treatment"}
    for label, stage in stages.items():
        assert stage["label"] == label and stage["noOverflow"] and stage["audioFactoryCount"] == 1
        assert stage["presentationQueryCalls"] == queries, "no search replay while assignment or Back changes"
        bar, state = stage["playerbar"], stage["nativePlayer"]
        assert bar["state"] == state["state"]
        if label != "initial":
            assert state["state"] == "paused" and not state["nativeSeeking"] and state["error"] is None
            assert all(state[key] == paused[key] for key in clock_identity) and stage["nativeQueue"] == queue
            assert abs(state["position"] - paused["position"]) < .04
            assert bar["playId"] == state["playId"] and bar["trackId"] == "1" and bar["position"] > 2
            assert bar["title"] == state["track"]["name"]
        if label in controls | treatments:
            treatment = label in treatments
            assert stage["footerCount"] == (3 if treatment else 0)
            assert stage["renderedTracks"] == 3
            assert stage["bestKeys"] == [f"artist:{index}" for index in range(100, 103 if treatment else 102)]
            assert stage["regularCandidateCount"] == (10 if treatment else 6)
            if treatment:
                visible = 10
            else:
                columns = stage["regularColumns"]
                assert isinstance(columns, int) and columns >= 1
                assert stage["regularPreview"] and stage["regularSingleRow"]
                visible = min(6, columns)
            assert stage["regularKeys"] == [f"artist:{index}" for index in range(200, 200 + visible)]
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


def verify_history_settings(root, repository, screenshots, windows):
    output = root / "first"
    failed = output / "failed.json"
    assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
    done = read(output / "complete.json")
    assert done["ok"] and done["stageCount"] == len(HISTORY_LABELS)
    assert done["realMusicRequests"] == done["realAccountWrites"] == 0
    paths = sorted(output.glob("ux_*.json"))
    assert [path.name for path in paths] == [
        f"ux_{index:02}_{label}.json" for index, label in enumerate(HISTORY_LABELS, 1)
    ]
    stages = list(map(read, paths))
    epoch = done["epoch"]
    assert [item["label"] for item in stages] == list(HISTORY_LABELS)
    assert [item["checked"] for item in stages] == [True, False, True, False, False, False, False]
    assert [item["dialog"] for item in stages] == [False, False, False, True, False, False, False]
    assert [item["saved"] for item in stages] == [False] * 5 + [True, True]
    assert [item["native"]["firstDisableConfirmed"] for item in stages] == [False] * 5 + [True, True]
    assert [item["native"]["enabled"] for item in stages] == [item["checked"] for item in stages]
    assert stages[4]["focusRestored"] and stages[5]["focusRestored"]
    assert all(item["nativePlayer"]["state"] == "idle" for item in stages)
    assert stages[-1]["native"] == stages[-2]["native"]

    component = [{"epoch": epoch, "action": {"kind": "read"}}, {"epoch": epoch, "action": {
        "kind": "set", "enabled": False, "confirmed_first_disable": True,
    }}]
    assert done["componentRequests"] == component
    assert [item["componentRequests"] for item in stages] == [component[:1]] * 5 + [component] * 2
    observations = done["observations"]
    assert len(observations) == 6 and all(item["epoch"] == epoch for item in observations)
    native_events = [item["value"] for item in observations]
    assert [item["enabled"] for item in native_events] == [False, True, False, True, False, False]
    assert [item["firstDisableConfirmed"] for item in native_events] == [False] * 5 + [True]
    revisions = [int(item["revision"]) for item in native_events]
    assert revisions == list(range(revisions[0], revisions[0] + 6))
    assert native_events[-1] == stages[-1]["native"]

    trace = done["signedSearch"]
    assert trace["transport"] == "MusicHttpClient" and trace["syntheticOnly"] and trace["sameProfileAccountRestore"]
    assert trace["footerCase"] == trace["serverMode"] == "t1" and trace["serverGeneration"] == 6
    modes = ("t1", "c", "t1", "c", "t1", "c", "t1")
    calls = trace["calls"]
    assert [call["path"] for call in calls] == ["/api/w/nuser/account/get"] + [ASSIGNMENT] * 7
    assert calls[0]["args"] == {}
    assert all(call["signedPcContextVerified"] and call["sameAccountCredential"]
               and call["responseCode"] == 200 for call in calls)
    for generation, (call, mode) in enumerate(zip(calls[1:], modes)):
        assert call["args"] == assignment_arguments(repository)
        assert call["serverMode"] == mode and call["serverGeneration"] == generation
    assert not done["presentationQueryCalls"], "settings observations must not search or re-run a result"
    commands = done["commands"]
    assert [command["mode"] for command in commands] == list(modes[1:])
    for index, command in enumerate(commands, 1):
        assert command["beforeFull"] == index and command["afterFull"] == index + 1
    assert commands[-1]["before"] == commands[-1]["after"] == native_events[-1]

    # Verify the local preference actually persisted and that no other account
    # or guest record was introduced by this opt-in proof.
    state = (root / "state" / "core.sqlite3").resolve(strict=True)
    with sqlite3.connect(state.as_uri() + "?mode=ro", uri=True) as database:
        rows = database.execute(
            "SELECT owner,key,value FROM state WHERE kind='play-history-v1' AND key LIKE 'sync-%'"
        ).fetchall()
    assert all(owner == "2" for owner, _, _ in rows)
    stored = {key: json.loads(value) for _, key, value in rows}
    assert stored == {
        "sync-enabled": False, "sync-first-off-confirmed": True, "sync-experiment-default": False,
        "sync-view-revision": revisions[-1], "sync-privacy-revision": revisions[-1] - 1,
    }, stored
    locale = None
    if not windows:
        locale = read(output / "locale.json")
        assert locale["numeric"] == "C"
        assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                   for key in ("messages", "characters"))
    pictures = len(list(output.glob("ux_*.png")))
    assert pictures == (len(HISTORY_LABELS) if screenshots else 0)
    return dict(ok=True, actualWebView=True, genuineSignedPcTransport=True, sameCredentialRestore=True,
                stageCount=len(HISTORY_LABELS), screenshots=pictures, fullAssignmentReads=7,
                committedHistoryEvents=6, componentReads=1, explicitLocalSaves=1, locale=locale,
                realMusicRequests=0, realAccountWrites=0)


def verify_preview_density(root, repository, screenshots):
    output = root / "first"
    failed = output / "failed.json"
    assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
    done = read(output / "complete.json")
    modes = [mode for mode, _, _, _ in PREVIEW_COHORTS]
    names = [f"ux_{index:02}_{mode}" for index, mode in enumerate(modes, 1)]
    capture_names = [label for name in names for label in (name, name + "_artist", name + "_mv")]
    assert done["ok"] and done["previewDensityVerified"] and done["stageCount"] == len(names)
    assert done["sectionStageCount"] == len(capture_names) - len(names)
    assert done["realMusicRequests"] == done["realAccountWrites"] == 0
    paths = sorted(output.glob("ux_*.json"))
    assert [path.stem for path in paths] == capture_names
    captures = {path.stem: read(path) for path in paths}
    stages = [captures[name] for name in names]
    assert all(captures[name]["stage"] == name for name in capture_names)
    trace = done["signedSearch"]
    assert trace["transport"] == "MusicHttpClient" and trace["syntheticOnly"] and trace["sameProfileAccountRestore"]
    assert trace["footerCase"] == "c" and trace["serverMode"] == "both" and trace["serverGeneration"] == 3
    calls = trace["calls"]
    assert [call["path"] for call in calls] == ["/api/w/nuser/account/get"] + [ASSIGNMENT] * 4, calls
    assert calls[0]["args"] == {}
    assert all(call["signedPcContextVerified"] and call["sameAccountCredential"]
               and call["responseCode"] == 200 for call in calls)
    for generation, (call, mode) in enumerate(zip(calls[1:], modes)):
        assert call["args"] == assignment_arguments(repository)
        assert call["serverMode"] == mode and call["serverGeneration"] == generation
    queries = [
        {"path": "/api/search/pc/result/tab", "args": {"keyword": "合成 V3"}},
        {"path": "/api/search/pc/complex/page/v3", "args": {
            "keyword": "合成 V3", "scene": "normal", "needCorrect": True,
            "channel": "typing", "bizQueryInfo": ""}},
    ]
    commands, events, projections = (done[key] for key in
                                     ("refreshCommands", "presentationEvents", "receiptProjections"))
    assert [item["mode"] for item in commands] == modes[1:]
    assert len(projections) == len(commands) == 3, projections
    paused, queue = stages[0]["nativePlayer"], stages[0]["nativeQueue"]
    assert paused["state"] == "paused" and paused["position"] > 2
    assert [row["track"]["id"] for row in queue["tracks"]] == ["1"]
    assert done["nativeQueue"] == queue and done["presentationQueryCalls"] == queries
    restored = done["restoredViews"]
    assert len(restored) == len(names)

    def assert_inside(rect, viewport):
        assert viewport["left"] - .5 <= rect["left"] < rect["right"] <= viewport["right"] + .5
        assert viewport["top"] + 1 <= rect["top"] < rect["bottom"] <= viewport["bottom"] - 1

    for index, (stage, (mode, expanded, dense, layout)) in enumerate(zip(stages, PREVIEW_COHORTS)):
        name = names[index]
        assert stage["mode"] == mode and stage["focusPreserved"] and stage["noOverflow"]
        assert abs(stage["scrollTop"] - 70) < 1
        assert restored[index]["stage"] == name and restored[index]["focusKey"] == "song:1"
        assert abs(restored[index]["scrollTop"] - stage["scrollTop"]) < 1
        assert stage["presentationQueryCalls"] == queries, "layout update must not replay search"
        assert stage["syntheticSearchCalls"] == done["syntheticSearchCalls"], "unexpected additional fixture search call"
        assert stage["audioFactoryCount"] == 1 and stage["nativeQueue"] == queue
        state, view = stage["nativePlayer"], stage["publicPresentation"]
        assert state["state"] == "paused" and state["track"]["id"] == "1"
        assert all(state[key] == paused[key] for key in ("playId", "entryId", "queueRevision"))
        assert abs(state["position"] - paused["position"]) < .04
        assert (view["searchExpanded"], view["searchDense"], view["searchFooter"]) == (expanded, dense, False)
        assert stage["bestKeys"] == [f"artist:{item}" for item in range(100, 103 if expanded else 102)]
        if index:
            command, projection = commands[index - 1], projections[index - 1]
            assert (command["beforeFull"], command["afterFull"]) == (index, index + 1)
            assert command["before"] == stages[index - 1]["publicPresentation"]
            assert command["after"] == view and view in events
            assert int(view["revision"]) > int(command["before"]["revision"])
            assert projection == {"epoch": view["epoch"], "receiptPresent": True,
                                  "revision": view["revision"], "replyRevision": view["revision"], "layout": layout}
        for kind, first in (("artist", 200), ("mv", 300)):
            row = stage[kind]
            gap = 16 if dense else 20
            span = (3 if expanded or dense else 4) if kind == "mv" else (2 if dense else 3)
            unit = 66 if expanded or kind == "mv" or not dense else 64
            columns = max(1, min(18 // span, (row["width"] + (20 if expanded else gap)) // (span * unit)))
            candidates = 10 if expanded else 6
            visible = min(candidates, columns)
            assert row["carousel"] == expanded and row["compact"] == dense
            assert row["rowPreview"] == (not expanded) and row["singleRow"]
            assert row["candidateCount"] == candidates and row["columns"] == columns
            assert row["columnGap"] == f"{gap}px"
            assert abs(row["firstTrackPx"] - ((row["width"] + gap) / columns - gap)) < 1.1
            assert row["visibleKeys"] == [f"{kind}:{item}" for item in range(first, first + visible)]
            assert row["allKeys"] == [
                f"{kind}:{item}" for item in range(first, first + (candidates if expanded else visible))
            ]
            capture = captures[f"{name}_{kind}"]
            assert capture["kind"] == kind and capture["layoutStage"] == name and capture["focusPreserved"]
            assert capture["nativeQueue"] == queue and capture["presentationQueryCalls"] == queries
            assert capture["audioFactoryCount"] == 1
            assert capture["syntheticSearchCalls"] == done["syntheticSearchCalls"]
            capture_player = capture["nativePlayer"]
            assert all(capture_player[key] == paused[key] for key in ("state", "playId", "entryId", "queueRevision"))
            assert abs(capture_player["position"] - paused["position"]) < .04
            frame = capture["frame"]
            assert frame["scrollTop"] > stage["scrollTop"]
            viewport, window_size = frame["viewport"], frame["windowSize"]
            assert 0 <= viewport["left"] < viewport["right"] <= window_size["width"]
            assert 0 <= viewport["top"] < viewport["bottom"] <= window_size["height"]
            assert_inside(frame["section"], viewport)
            assert_inside(frame["heading"], viewport)
            assert [card["key"] for card in frame["cards"]] == row["visibleKeys"]
            for card in frame["cards"]:
                assert_inside(card["bounds"], viewport)
    assert all(stages[1][kind]["columns"] > stages[0][kind]["columns"] for kind in ("artist", "mv")), (
        "fixture width must visibly distinguish the two official single-row spans"
    )
    state = done["nativePlayer"]
    assert state["state"] == "paused" and all(state[key] == paused[key] for key in ("playId", "entryId", "queueRevision"))
    assert abs(state["position"] - paused["position"]) < .04
    locale = read(output / "locale.json")
    assert locale["numeric"] == "C"
    assert all(locale[key].casefold().replace("-", "") == "en_us.utf8" for key in ("messages", "characters"))
    pictures = sorted(output.glob("ux_*.png"))
    markers = sorted(output.glob("ux_*.captured"))
    expected = capture_names if screenshots else []
    assert [path.stem for path in pictures] == [path.stem for path in markers] == expected
    assert all(bool(capture.get("captureHandshake")) == screenshots for capture in captures.values())
    for path, marker in zip(pictures, markers):
        assert marker.read_text(encoding="utf-8") == "captured\n"
        with path.open("rb") as image:
            header = image.read(24)
        assert header[:16] == b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR" and len(header) == 24
        assert min(struct.unpack(">II", header[16:])) > 0
    return dict(ok=True, actualWebView=True, genuineSignedPcTransport=True, sameCredentialRestore=True,
                stageCount=len(names), sectionStageCount=len(capture_names) - len(names),
                screenshots=len(pictures), fullAssignmentReads=4, comprehensiveSearchReads=2,
                nativeReceiptProjections=3, nativeNullPlaybackVerified=True, locale=locale,
                layouts=[{"cohort": mode, "more2": expanded, "efficiency": dense,
                          "artistColumns": stage["artist"]["columns"], "mvColumns": stage["mv"]["columns"]}
                         for stage, (mode, expanded, dense, _) in zip(stages, PREVIEW_COHORTS)],
                realMusicRequests=0, realAccountWrites=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--history-settings", action="store_true",
                      help="observe the open history sync setting and explicit confirmation")
    mode.add_argument("--preview-density", action="store_true",
                      help="verify four signed layout cohorts on one cached search")
    args = parser.parse_args()
    windows = os.name == "nt"
    if windows and args.preview_density:
        parser.error("--preview-density is the sealed Linux WebView proof")
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
    if args.history_settings:
        env["CUMULUS_HISTORY_SETTINGS_COHORT_PROOF"] = "1"
        env["CUMULUS_UX_SCRIPT"] = str(repository / "desktop/examples/history_sync_cohort_ui_probe.js")
    if args.preview_density:
        env["CUMULUS_SEARCH_FOOTER_PROOF"] = "c"
        env["CUMULUS_SEARCH_PREVIEW_DENSITY_PROOF"] = "1"
        env["CUMULUS_UX_SCRIPT"] = str(repository / "desktop/examples/search_preview_density_ui_probe.js")
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
    if args.preview_density:
        report = verify_preview_density(root, repository, args.screenshots)
    else:
        validator = verify_history_settings if args.history_settings else verify
        report = validator(root, repository, args.screenshots, windows)
    assert code == 0, (code, str(root / "host.log"))
    report["windowsMedia"] = windows_media
    (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
