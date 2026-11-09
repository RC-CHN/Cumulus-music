"""Sealed Linux WebView cases for the priority Copyright startup notification."""
import argparse
import json
import os
from pathlib import Path

from daily_notification_smoke import read, signed_ready_before_page
from motion_smoke import launch
from search_signed_trace import assignment_arguments

CASES = ("existing", "play", "settings", "snooze", "empty", "error", "create_only", "fullscreen")
POPUP = "/api/pc/popup/copyright"
SONGS = "/api/v3/song/detail"
DAILY = "/api/v3/discovery/recommend/songs"
SETTINGS = "/api/user/setting"
MEDIA = "/api/song/enhance/player/url/v1"
PLAYLIST = "/api/v6/playlist/detail"


def verify(directory, mode, expected, screenshots):
    output = directory / "first"
    complete = read(output / "complete.json")
    assert complete["ok"] and complete["copyrightMode"] == mode
    labels = (["daily_fallback"] if mode in {"empty", "error"} else
              ["suppressed"] if mode in {"create_only", "fullscreen"} else
              ["offered", "playing" if mode == "play" else mode])
    files = sorted(output.glob("ux_*.json"))
    assert [path.stem for path in files] == [f"ux_{i:02}_{label}" for i, label in enumerate(labels, 1)]
    stages = dict(zip(labels, map(read, files)))
    assert all(row["noOverflow"] for row in stages.values())
    assert complete["count"] == len(labels)
    for frame in [*stages.values(), complete]:
        signed_ready_before_page(frame["signedSearch"], expected)
    fixture = complete["dailyNotificationFixture"]
    wire = fixture["copyrightWire"]
    expected_paths = [] if mode == "fullscreen" else [POPUP] if mode in {"empty", "error"} else [POPUP, SONGS]
    assert [row["path"] for row in wire] == expected_paths, wire
    assert all(row["signedPcContextVerified"] and row["sameAccountCredential"] for row in wire)
    if wire:
        assert wire[0]["args"] == {}
        assert wire[0]["responseCode"] == (500 if mode == "error" else 200)
    if len(wire) == 2:
        assert wire[1]["args"] == {"c": '[{"id":"3","v":0},{"id":"5","v":0}]', "trialMode": -1}
        assert wire[1]["responseCode"] == 200
    calls = fixture["calls"]
    assert all(row["path"] in {DAILY, SETTINGS, MEDIA, PLAYLIST} for row in calls), calls
    settings = [row for row in calls if row["path"] == SETTINGS]
    assert len(settings) == (1 if mode in {"settings", "empty", "error"} else 0), settings
    if mode in {"empty", "error"}:
        assert stages["daily_fallback"]["noticeKind"] == "daily"
        assert len(fixture["history"]["shown"]) == 1 and fixture["copyrightHistory"] is None
    elif mode in {"create_only", "fullscreen"}:
        assert stages["suppressed"]["noticeKind"] is None
        assert fixture["history"] is None and fixture["copyrightHistory"] is None
    else:
        first = stages["offered"]
        assert first["noticeKind"] == "copyright" and first["songs"] == ["合成歌曲 3", "合成歌曲 5"]
        assert first["safeArt"] == "https://p1.music.126.net/fixture-copyright.jpg?param=320y320"
        history = fixture["copyrightHistory"]
        assert len(history["shown"]) == 1 and fixture["history"] is None
        assert (history["snoozed_at"] is not None) is (mode == "snooze")
        assert history["enabled"] is (mode != "settings")
        assert stages[labels[-1]]["noticeKind"] is None
    if mode == "existing":
        assert [row["args"]["id"] for row in calls if row["path"] == PLAYLIST] == ["81"]
        assert stages[mode]["heading"] == "合成回归歌单"
    if mode == "settings":
        assert stages[mode]["hasCopyrightSetting"] is False and stages[mode]["hasDailySetting"] is True
    if mode == "play":
        initial, result = stages["offered"], stages["playing"]
        assert [row["track"]["id"] for row in initial["nativeQueue"]["tracks"]] == ["1", "2", "3", "4"]
        assert initial["nativePlayer"]["state"] == "playing" and initial["nativePlayer"]["track"]["id"] == "2"
        assert [row["track"]["id"] for row in result["nativeQueue"]["tracks"]] == ["1", "2", "3", "5", "4"]
        assert result["nativePlayer"]["state"] == "playing" and result["nativePlayer"]["track"]["id"] == "3"
        assert result["nativePlayer"]["playId"] != initial["nativePlayer"]["playId"]
        assert result["nativePlayer"]["position"] > .25 and result["audioFactoryCount"] == 1
        assert [i for i, source in enumerate(fixture["sources"]) if source.get("scene") == "copyrightReversionPush"] == [2,3]
        assert any(call["path"] == MEDIA and call["args"]["trialMode"] == -1
                   and json.loads(call["args"]["ids"]) in ([3],["3"]) for call in calls), calls
    elif mode != "play":
        assert complete["nativePlayer"]["state"] == "idle"
        assert not any(row["path"] == MEDIA for row in calls)
    preflight = read(output / "daily-preflight.json")
    assert preflight[0]["armed"] is True, preflight
    if mode == "fullscreen":
        assert preflight[-1]["fullscreen"] or preflight[-1]["fullscreenGeometry"], preflight
    else:
        assert preflight[-1]["focused"] and not preflight[-1]["fullscreen"], preflight
    locale = read(output / "locale.json")
    assert locale["numeric"] == "C"
    assert all(locale[key].casefold().replace("-", "") == "en_us.utf8" for key in ("messages", "characters"))
    images = len(list(output.glob("ux_*.png")))
    assert images == (len(labels) if screenshots else 0)
    return dict(mode=mode, states=labels, screenshots=images, copyrightWire=wire,
                calls=[row["path"] for row in calls], actualWebView=True,
                nativeNullPlayback=mode=="play", realMusicRequests=0, realAccountWrites=0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--case", action="append", choices=CASES)
    parser.add_argument("--screenshots", action="store_true")
    args = parser.parse_args()
    if os.name != "posix" or not os.environ.get("DISPLAY"):
        parser.error("run in an isolated X11 display with a real window manager")
    binary, assets = args.binary.resolve(strict=True), args.frontend_dist.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    repo = Path(__file__).resolve().parents[2]
    source = (repo / "desktop/examples/copyright_notification_ui_probe.js").read_text(encoding="utf-8")
    expected = assignment_arguments(repo)
    base = {key: value for key, value in os.environ.items() if not key.startswith("CUMULUS_")}
    cases = []
    for mode in args.case or CASES:
        directory = root / mode
        directory.mkdir()
        script = directory / "probe.js"
        script.write_text(f"window.__copyrightNotificationCase={json.dumps(mode)};\n" + source, encoding="utf-8")
        env = dict(base, CUMULUS_PROOF_DIR=str(directory), CUMULUS_UX_SCRIPT=str(script),
                   CUMULUS_PROOF_FRONTEND_DIST=str(assets), CUMULUS_MOTION_PROOF="1",
                   CUMULUS_DAILY_NOTIFICATION_PROOF="copyright_" + mode,
                   GDK_BACKEND="x11", LC_ALL="en_US.UTF-8")
        for name in ("DATA", "CONFIG", "CACHE", "RUNTIME"):
            path = directory / name.lower()
            path.mkdir(mode=0o700)
            env[f"XDG_{name}_{'DIR' if name == 'RUNTIME' else 'HOME'}"] = str(path)
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            code = launch(binary, env, log, directory / "first", args.screenshots)
        failure = directory / "first/failed.json"
        assert not failure.exists(), failure.read_text(encoding="utf-8") if failure.exists() else ""
        assert code == 0, (mode, code, str(directory / "host.log"))
        report = verify(directory, mode, expected, args.screenshots)
        cases.append(report)
        print(json.dumps(report, ensure_ascii=False), flush=True)
    (root / "report.json").write_text(json.dumps({"ok": True, "cases": cases}, ensure_ascii=False, indent=2)
                                      + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
