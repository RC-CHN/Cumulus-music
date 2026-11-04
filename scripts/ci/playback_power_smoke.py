"""Production playback power UI, IPC, SQLite and native audio, with a sealed fake OS sink."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import sys

from motion_smoke import launch

PHASES = {
    "initial": {
        "paused_default": ("paused", None, False, None),
        "enabled_while_paused": ("paused", True, True, "clear"),
        "playing_acquires": ("playing", True, True, "keep"),
        "write_failure_keeps_hold": ("playing", True, True, None),
        "disabled_while_playing": ("playing", False, False, "clear"),
        "enabled_while_playing": ("playing", True, True, "keep"),
        "pause_releases": ("paused", True, True, "clear"),
        "second_resume_acquires": ("playing", True, True, "keep"),
        "stop_releases": ("idle", True, True, "clear"),
    },
    "reopen": {
        "reopened_checked_without_hold": ("paused", True, True, None),
        "reopened_playing_acquires": ("playing", True, True, "keep"),
        "reopened_stop_releases": ("idle", True, True, "clear"),
    },
    "control": {
        "hidden": ("paused", True, None, None),
        "hidden_playing": ("playing", True, None, "keep"),
        "hidden_stop": ("idle", True, None, "clear"),
    },
    "unsupported": {
        "hidden": ("paused", True, None, None),
        "hidden_playing": ("playing", True, None, None),
        "hidden_stop": ("idle", True, None, None),
    },
}


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def audit(root, screenshots, windows_media=None):
    traces = {}
    locales = {}
    for phase, expected in PHASES.items():
        directory = root / phase
        complete = read(directory / "complete.json")
        assert complete["ok"] and complete["phase"] == phase and complete["stageCount"] == len(expected)
        assert complete["realMusicRequests"] == complete["realAccountWrites"] == complete["realOsPowerCalls"] == 0
        assert complete["devicePreference"] is True
        assert complete["experimentReads"] == (0 if phase == "unsupported" else 1)
        previous = None
        for index, (label, (state, preference, checked, added)) in enumerate(expected.items(), 1):
            path = directory / f"ux_{index:02}_{label}.json"
            row = read(path)
            assert row["devicePreference"] is preference, (phase, label, row)
            assert row["visible"] == (checked is not None) and row["checked"] is checked, (phase, label, row)
            player = row["nativePlayer"]
            assert player["state"] == state and not player["error"], (phase, label, player)
            if state in {"playing", "paused"}:
                assert player["track"]["id"] == "2" and player["position"] >= 0, (phase, label, player)
            trace = row["powerRequests"]
            if previous is None:
                assert all(item == "clear" for item in trace)
                assert bool(trace) == (phase != "unsupported"), (phase, trace)
            else:
                assert trace == previous + ([added] if added else []), (phase, label, trace, previous)
            previous = trace
            if label == "write_failure_keeps_hold":
                assert row["error"] and not row["status"] and row["focus"] == "重新读取播放设置"
            if label in {"enabled_while_paused", "enabled_while_playing", "disabled_while_playing"}:
                assert row["status"] == "设置已更新" and row["focus"] == "playback-keep-awake"
            if screenshots:
                assert path.with_suffix(".png").is_file()
        assert complete["powerRequests"] == previous, (phase, complete)
        traces[phase] = previous
        if windows_media is None:
            locale = read(directory / "locale.json")
            assert locale["numeric"] == "C"
            assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                       for key in ("messages", "characters"))
            locales[phase] = locale
    with sqlite3.connect((root / "state/core.sqlite3").as_uri() + "?mode=ro", uri=True) as db:
        rows = db.execute("SELECT owner, key, value FROM state WHERE kind='playback-power-v1'").fetchall()
        assert len(rows) == 1 and rows[0][:2] == ("local-device", "enabled") and json.loads(rows[0][2]) is True
        assert not db.execute("SELECT name FROM sqlite_master WHERE type='trigger' AND name='reject_power'").fetchall()
    count = sum(map(len, PHASES.values()))
    return dict(ok=True, actualWebView=True,
                webViewRuntime="WebView2" if windows_media else "GTK/WebKit",
                nativeLibmpvNull=True, windowsMediaRuntime=windows_media, stateCount=count,
                screenshots=count if screenshots else 0, processLifetimes=len(PHASES),
                deviceLocalPersistence=True, sealedPowerSink=True, realOsPowerCalls=0,
                realMusicRequests=0, realAccountWrites=0, locales=locales, traces=traces)


def windows_runtime(binary, stage):
    # The production loader intentionally ignores PATH. A sibling takes
    # precedence over the environment; refuse a shadowing, unpinned DLL.
    stage = stage.resolve(strict=True)
    assert stage.is_dir(), "staged Windows media runtime must be a directory"
    names = ("libmpv-2.dll", "mpv-2.dll")
    candidate = next((stage / name for name in names if (stage / name).is_file()), None)
    assert candidate, "staged Windows media runtime has no libmpv DLL"
    assert stage == binary.parent or not any((binary.parent / name).is_file() for name in names), (
        "a libmpv DLL beside the probe would override the staged runtime"
    )
    with candidate.open("rb") as dll:
        return stage, {"file": candidate.name, "sha256": hashlib.file_digest(dll, "sha256").hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path, help="exact staged Windows native media directory")
    parser.add_argument("--screenshots", action="store_true", help="optional Linux GTK capture")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if not windows and sys.platform != "linux":
        parser.error("the playback power WebView proof supports Linux and Windows")
    if windows and (not args.runtime or args.screenshots):
        parser.error("Windows requires --runtime and does not support GTK screenshot capture")
    if not windows and args.runtime:
        parser.error("--runtime identifies the Windows package stage only")
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    windows_media = None
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_"))
           and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    if windows:
        stage, windows_media = windows_runtime(binary, args.runtime)
        env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profile_dirs = [("APPDATA", "roaming"), ("LOCALAPPDATA", "local"), ("TEMP", "temp")]
    else:
        env["LC_ALL"] = "en_US.UTF-8"
        profile_dirs = [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                        ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")]
    root.mkdir(parents=True, exist_ok=False)
    env["CUMULUS_PROOF_DIR"] = str(root)
    for key, name in profile_dirs:
        directory = root / "os-profile" / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if windows:
        env["TMP"] = env["TEMP"]
    if args.frontend_dist:
        env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
    for phase in PHASES:
        with (root / f"{phase}.log").open("x", encoding="utf-8") as log:
            code = launch(binary, {**env, "CUMULUS_POWER_PROOF_PHASE": phase},
                          log, root / phase, args.screenshots)
        failed = root / phase / "failed.json"
        assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
        assert code == 0, (phase, code)
    result = audit(root, args.screenshots, windows_media)
    (root / "audit.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
