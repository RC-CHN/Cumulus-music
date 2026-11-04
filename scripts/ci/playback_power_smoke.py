"""Production playback power UI, IPC, SQLite and native audio, with a sealed fake OS sink."""
import argparse
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


def audit(root, screenshots):
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
        locale = read(directory / "locale.json")
        assert locale["numeric"] == "C"
        assert all(locale[key].casefold().replace("-", "") == "en_us.utf8" for key in ("messages", "characters"))
        locales[phase] = locale
    with sqlite3.connect((root / "state/core.sqlite3").as_uri() + "?mode=ro", uri=True) as db:
        rows = db.execute("SELECT owner, key, value FROM state WHERE kind='playback-power-v1'").fetchall()
        assert len(rows) == 1 and rows[0][:2] == ("local-device", "enabled") and json.loads(rows[0][2]) is True
        assert not db.execute("SELECT name FROM sqlite_master WHERE type='trigger' AND name='reject_power'").fetchall()
    count = sum(map(len, PHASES.values()))
    return dict(ok=True, actualLinuxWebView=True, nativeLibmpvNull=True, stateCount=count,
                screenshots=count if screenshots else 0, processLifetimes=len(PHASES),
                deviceLocalPersistence=True, sealedPowerSink=True, realOsPowerCalls=0,
                realMusicRequests=0, realAccountWrites=0, locales=locales, traces=traces)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    args = parser.parse_args()
    assert sys.platform == "linux", "This sealed actual WebView proof currently targets Linux"
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith("CUMULUS_")}
    env.update(CUMULUS_PROOF_DIR=str(root), LC_ALL="en_US.UTF-8")
    for key, name in [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                      ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")]:
        directory = root / "os-profile" / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.frontend_dist:
        env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
    for phase in PHASES:
        with (root / f"{phase}.log").open("x", encoding="utf-8") as log:
            code = launch(args.binary.resolve(), {**env, "CUMULUS_POWER_PROOF_PHASE": phase},
                          log, root / phase, args.screenshots)
        failed = root / phase / "failed.json"
        assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
        assert code == 0, (phase, code)
    result = audit(root, args.screenshots)
    (root / "audit.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
