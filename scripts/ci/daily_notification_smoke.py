"""Run the traditional startup Daily notification in an isolated Linux WebView."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch
from search_signed_trace import ACCOUNT, ASSIGNMENT, assignment_arguments, signed_assignment

CASES = ("play", "settings", "snooze", "disable", "close", "details", "privacy", "fullscreen", "control")
SUPPRESSED = {"privacy", "fullscreen", "control"}
SETTINGS = "/api/user/setting"
DAILY = "/api/v3/discovery/recommend/songs"
MEDIA = "/api/song/enhance/player/url/v1"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def signed_ready_before_page(trace, expected):
    calls = trace["calls"]
    assert [call["path"] for call in calls] == [ACCOUNT, ASSIGNMENT, ACCOUNT], calls
    signed_assignment({**trace, "calls": calls[:2]}, "t1", expected)
    assert calls[2] == calls[0], "WebView restores the same sealed native account without another assignment"


def verify(directory, mode, expected, screenshots):
    output = directory / "first"
    complete = read(output / "complete.json")
    assert complete["ok"] and complete["dailyNotificationMode"] == mode
    labels = (["suppressed"] if mode in SUPPRESSED else ["offered", {
        "play": "playing", "close": "closed",
    }.get(mode, mode)])
    files = sorted(output.glob("ux_*.json"))
    assert [path.stem for path in files] == [f"ux_{i:02}_{label}" for i, label in enumerate(labels, 1)]
    states = dict(zip(labels, map(read, files)))
    assert complete["count"] == len(states)
    signed_ready_before_page(complete["signedSearch"], expected)
    assert all(state["noOverflow"] for state in states.values())
    for row in states.values():
        signed_ready_before_page(row["signedSearch"], expected)
    locale = read(output / "locale.json")
    assert locale["numeric"] == "C"
    assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
               for key in ("messages", "characters")), locale
    native_preflight = read(output / "daily-preflight.json")
    assert native_preflight[0]["elapsedMs"] < 1000, native_preflight
    assert native_preflight[0]["armed"] is (mode != "control"), native_preflight
    before_due = next(frame for frame in native_preflight if frame["elapsedMs"] >= 4000)
    assert before_due["planError"] is None, native_preflight
    assert before_due["armed"] is (mode != "control"), native_preflight
    if mode == "fullscreen":
        assert before_due["fullscreen"] or before_due["fullscreenGeometry"], native_preflight
    else:
        assert before_due["focused"] and before_due["visible"] and not before_due["minimized"], native_preflight
        assert before_due["fullscreen"] is False and before_due["fullscreenGeometry"] is False, native_preflight
    fixture = complete["dailyNotificationFixture"]
    calls, history = fixture["calls"], fixture["history"]
    assert all(call["path"] in (SETTINGS, DAILY, MEDIA) for call in calls)
    assert len([call for call in calls if call["path"] == DAILY]) == (2 if mode in {"play", "details"} else 1), calls
    for call in calls:
        if call["path"] == DAILY:
            assert call["args"] == {"limit": 30}
    settings = [call for call in calls if call["path"] == SETTINGS]
    assert len(settings) in ({1, 2} if mode == "settings" else {0} if mode in {"control", "fullscreen"} else {1}), calls
    assert all(call["args"] == {} for call in settings)
    media = [call["args"] for call in calls if call["path"] == MEDIA]
    if mode in SUPPRESSED:
        assert history is None and fixture["sources"] is None and not media
        assert states["suppressed"]["noticeTitle"] is None
        assert complete["nativePlayer"]["state"] == "idle"
        assert states["suppressed"]["fullscreen"] == (mode == "fullscreen")
    else:
        offered = states["offered"]
        assert offered["noticeTitle"] == "每日推荐" and len(offered["songs"]) == 3
        assert offered["nativePlayer"]["state"] == "idle"
        assert [call["path"] for call in offered["dailyNotificationFixture"]["calls"]] == [DAILY, SETTINGS]
        assert len(history["shown"]) == 1 and history["enabled"] is (mode != "disable")
        assert (history["snoozed_at"] is not None) is (mode == "snooze")
        assert states[labels[-1]]["noticeTitle"] is None
        if mode == "details":
            assert states[mode]["heading"].endswith("每日推荐")
        if mode == "settings":
            assert states[mode]["focused"] == "settings-desktop-notifications" and states[mode]["preference"]
        if mode == "play":
            playing = states["playing"]
            assert playing["nativePlayer"]["state"] == "playing" and playing["nativePlayer"]["position"] > .2
            assert playing["nativePlayer"]["track"]["id"] == "1" and playing["audioFactoryCount"] == 1
            assert [row["track"]["id"] for row in playing["nativeQueue"]["tracks"]] == ["1", "2", "3", "4"]
            assert len(fixture["sources"]) == 4
            assert all(source["scene"] == "dailyPush" and source["sourceData"]["id"] == "2"
                       and source["sourceData"]["day"].endswith("Z") for source in fixture["sources"])
            assert media and all(call["trialMode"] == 35 for call in media), media
        else:
            assert not media and fixture["sources"] is None
            assert complete["nativePlayer"]["state"] == "idle"
    images = len(list(output.glob("ux_*.png")))
    assert images == (len(labels) if screenshots else 0)
    return dict(mode=mode, states=labels, screenshots=images, calls=[call["path"] for call in calls],
                signedAccountAssignment=True, actualWebView=True, nativeNullPlayback=mode == "play",
                foregroundWindowOnly=True, nativePreflight=before_due, locale=locale,
                realMusicRequests=0, realAccountWrites=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--case", action="append", choices=CASES)
    parser.add_argument("--screenshots", action="store_true")
    args = parser.parse_args()
    if os.name != "posix" or not os.environ.get("DISPLAY"):
        parser.error("run with an isolated X11 display and a real window manager")
    binary, assets = args.binary.resolve(strict=True), args.frontend_dist.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    repo = Path(__file__).resolve().parents[2]
    script = (repo / "desktop/examples/daily_notification_ui_probe.js").read_text(encoding="utf-8")
    expected = assignment_arguments(repo)
    base = {key: value for key, value in os.environ.items() if not key.startswith("CUMULUS_")}
    cases = []
    for mode in args.case or CASES:
        directory = root / mode
        directory.mkdir()
        source = directory / "probe.js"
        source.write_text(f"window.__dailyNotificationCase={json.dumps(mode)};\n" + script, encoding="utf-8")
        env = dict(base, CUMULUS_PROOF_DIR=str(directory), CUMULUS_UX_SCRIPT=str(source),
                   CUMULUS_PROOF_FRONTEND_DIST=str(assets), CUMULUS_MOTION_PROOF="1",
                   CUMULUS_DAILY_NOTIFICATION_PROOF=mode, GDK_BACKEND="x11", LC_ALL="en_US.UTF-8")
        for name in ("DATA", "CONFIG", "CACHE", "RUNTIME"):
            path = directory / name.lower()
            path.mkdir(mode=0o700)
            env[f"XDG_{name}_{'DIR' if name == 'RUNTIME' else 'HOME'}"] = str(path)
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            code = launch(binary, env, log, directory / "first", args.screenshots)
        failed = directory / "first/failed.json"
        assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
        assert code == 0, (mode, code, str(directory / "host.log"))
        result = verify(directory, mode, expected, args.screenshots)
        cases.append(result)
        print(json.dumps(result, ensure_ascii=False), flush=True)
    (root / "report.json").write_text(json.dumps({"ok": True, "cases": cases}, ensure_ascii=False, indent=2)
                                      + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
