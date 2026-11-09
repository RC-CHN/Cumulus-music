"""Actual WebView/IPC with a sealed synthetic endpoint, plus the Linux unsupported branch."""
import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime

CASES = {
    "synthetic": {"initial": "37%", "muted": "0%", "unavailable": None,
                  "recovered": "84%", "reopened": "84%"},
    "unsupported": {"unsupported": None},
}

SPATIAL = {
    "initial": "未启用或设备不支持",
    "muted": "已开启（可能影响声音效果）",
    "unavailable": "暂时无法读取系统空间音效，请刷新重试。",
    "recovered": "已开启（可能影响声音效果）",
    "reopened": "已开启（可能影响声音效果）",
    "unsupported": None,
}


DIAGNOSIS_CASES = {
    "synthetic": {
        "below_threshold": (False, 79, 0, None),
        "muted": (False, 80, 1, None),
        "at_system_threshold": (False, 86, 2, None),
        "decrease": (False, 82, 2, None),
        "low_system_volume": (True, 85, 3, None),
        "automatic_close": (False, 85, 3, None),
        "spatial_unavailable": (False, 87, 4, None),
        "spatial_active": (True, 90, 5, None),
        "settings": (False, 90, 6, "84%"),
        "remembered_cooldown": (False, 92, 7, None),
    },
    "unsupported": {
        "below_threshold": (False, 79, 0, None),
        "unsupported_after_increase": (False, 80, 0, None),
    },
}


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path, help="exact staged Windows native media directory")
    parser.add_argument("--screenshots", action="store_true", help="optional Linux GTK capture")
    parser.add_argument("--diagnosis", action="store_true", help="exercise the player volume diagnosis")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if not windows and sys.platform != "linux":
        parser.error("the system master volume WebView proof supports Linux and Windows")
    if windows and (not args.runtime or args.screenshots):
        parser.error("Windows requires --runtime and does not support GTK screenshot capture")
    if not windows and args.runtime:
        parser.error("--runtime identifies the Windows package stage only")
    binary = args.binary.resolve(strict=True)
    windows_media = None
    base_env = {key: value for key, value in os.environ.items()
                if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_"))
                and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    if windows:
        stage, windows_media = windows_runtime(binary, args.runtime)
        base_env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profile_dirs = [("APPDATA", "roaming"), ("LOCALAPPDATA", "local"), ("TEMP", "temp")]
    else:
        base_env["LC_ALL"] = "en_US.UTF-8"
        profile_dirs = [("XDG_DATA_HOME", "data"), ("XDG_CONFIG_HOME", "config"),
                        ("XDG_CACHE_HOME", "cache"), ("XDG_RUNTIME_DIR", "runtime")]
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    probe_name = "volume_diagnosis" if args.diagnosis else "system_master_volume"
    probe = Path(__file__).resolve().parents[2] / "desktop/examples" / f"{probe_name}_ui_probe.js"
    locales = {}
    # A Windows run is synthetic-only: never fall back to the system reader.
    all_cases = DIAGNOSIS_CASES if args.diagnosis else CASES
    cases = {"synthetic": all_cases["synthetic"]} if windows else all_cases
    for case, expected in cases.items():
        directory = root / case
        directory.mkdir()
        env = {**base_env, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_MOTION_PROOF": "1",
               "CUMULUS_SYSTEM_MASTER_VOLUME_PROOF": case}
        for key, name in profile_dirs:
            isolated = directory / "os-profile" / name
            isolated.mkdir(parents=True, mode=0o700)
            env[key] = str(isolated)
        if windows:
            env["TMP"] = env["TEMP"]
        if args.frontend_dist:
            env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
        script = directory / "probe.js"
        flag = "__volumeDiagnosisUnsupported" if args.diagnosis else "__systemMasterVolumeUnsupported"
        script.write_text(f"window.{flag}={json.dumps(case == 'unsupported')};\n"
                          + probe.read_text(encoding="utf-8"), encoding="utf-8")
        env["CUMULUS_UX_SCRIPT"] = str(script)
        output = directory / "first"
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            code = launch(binary, env, log, output, args.screenshots)
        failed = output / "failed.json"
        assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
        assert code == 0, (case, code)
        if not windows:
            locale = read(output / "locale.json")
            assert locale["numeric"] == "C"
            assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                       for key in ("messages", "characters"))
            locales[case] = locale
        complete = read(output / "complete.json")
        assert complete["ok"] and complete["stageCount"] == len(expected)
        assert complete["realOsWrites"] == complete["realMusicRequests"] == complete["realAccountWrites"] == 0
        baseline = None
        for index, (label, observation) in enumerate(expected.items(), 1):
            path = output / f"ux_{index:02}_{label}.json"
            row = read(path)
            player = row["nativePlayer"]
            assert player["state"] == "paused" and player["playId"] == row["playId"] and not player["error"]
            if args.diagnosis:
                visible, volume, reads, percent = observation
                assert (row["visible"], row["volume"], row["outputPercent"]) == (visible, volume, percent), (label, row)
                for key in ("systemMasterVolumeReads", "systemSpatialSoundReads", "volumeReadsBeforeSpatial"):
                    assert row.get(key, 0) == reads, (label, key, row)
                assert player["volume"] == volume, (label, row)
                current = tuple(row[key] for key in ("playId", "entryId", "queueRevision", "position"))
                assert all(value is not None for value in current)
                assert tuple(player[key] for key in ("playId", "entryId", "queueRevision", "position")) == current
                baseline = baseline or current
                assert current == baseline, (label, row)
                if label == "settings":
                    assert row["focused"] == "settings-audio-output", row
            else:
                assert row["visible"] == (case == "synthetic") and row["percent"] == observation, (label, row)
                assert row["spatial"] == SPATIAL[label], (label, row)
                assert abs(player["position"] - row["pausedAt"]) < 0.2
                if case == "synthetic":
                    assert row["systemMasterVolumeReads"] == index, (label, row)
                    assert row["systemSpatialSoundReads"] == index, (label, row)
                    assert row["volumeReadsBeforeSpatial"] == index, (label, row)
                if label in {"muted", "unavailable", "recovered"}:
                    assert row["focused"] == "刷新音频输出设备", (label, row)
                if label == "recovered":
                    assert row["width"] == 760 and row["theme"] == "dark"
            if args.screenshots:
                assert path.with_suffix(".png").is_file()
    count = sum(map(len, cases.values()))
    result = dict(ok=True, actualWebView=True, webViewRuntime="WebView2" if windows else "GTK/WebKit",
                  nativeIpc=True, nativeLibmpvNull=True, windowsMediaRuntime=windows_media,
                  playerVolumeDiagnosis=args.diagnosis,
                  readOnlySpatialSound=True,
                  stateCount=count, screenshots=count if args.screenshots else 0,
                  syntheticWindowsEndpoint=True, nativeLinuxUnsupported=not windows,
                  physicalWindowsReadVerified=False, realOsWrites=0, realMusicRequests=0,
                  realAccountWrites=0, locales=locales)
    (root / "audit.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
