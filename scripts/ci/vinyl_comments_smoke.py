"""Verify the original vinyl comment mode and stock route in a real WebView."""

import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime

COUNT = "/api/resource/commentInfo/list"
LATEST = "/api/v1/resource/comments/R_SO_4_1"
HOT = "/api/v1/resource/hotcomments/R_SO_4_1"


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if sys.platform not in {"linux", "win32"} or windows != bool(args.runtime) or (windows and args.screenshots):
        parser.error("Windows needs --runtime; screenshots are Linux only")
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_")) and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    if windows:
        stage, _ = windows_runtime(binary, args.runtime)
        env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profiles = ("APPDATA", "LOCALAPPDATA", "TEMP", "TMP")
    else:
        env["LC_ALL"] = "en_US.UTF-8"
        profiles = ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR")
    if args.frontend_dist:
        env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
    source = (Path(__file__).resolve().parents[2] / "desktop/examples/vinyl_comments_ui_probe.js").read_text(encoding="utf-8")
    expected = {
        "t1": ("standard_before_read", "embedded_first", "embedded_append", "return_focus", "hot_route", "zero_count"),
        "c": ("control_route",),
    }
    summary = {}
    for group, names in expected.items():
        directory = root / group
        directory.mkdir()
        script = directory / "probe.js"
        script.write_text(f"window.__vinylCommentControl={json.dumps(group == 'c')};\n" + source,
                          encoding="utf-8")
        run = {**env, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_MOTION_PROOF": "1",
               "CUMULUS_VINYL_COMMENTS_PROOF": group, "CUMULUS_UX_SCRIPT": str(script)}
        for key in profiles:
            isolated = directory / key
            isolated.mkdir(mode=0o700)
            run[key] = str(isolated)
        output = directory / "first"
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            status = launch(binary, run, log, output, args.screenshots)
        failed = output / "failed.json"
        assert not failed.exists(), read(failed) if failed.exists() else None
        assert status == 0 and read(output / "complete.json")["stageCount"] == len(names)
        rows = {path.stem.split("_", 2)[2]: read(path) for path in sorted(output.glob("ux_*.json"))}
        assert tuple(rows) == names
        for row in rows.values():
            assert row["nativePlayer"]["playId"] == row["playId"] and row["nativePlayer"]["state"] == "paused"
        if group == "t1":
            assert [call["path"] for call in rows["standard_before_read"]["vinylCommentsCalls"]] == [COUNT]
            calls = rows["zero_count"]["vinylCommentsCalls"]
            assert [call["path"] for call in calls] == [COUNT, LATEST, LATEST, HOT, COUNT]
            for call, offset in zip(calls[1:3], [0, 60]):
                assert call["args"] == dict(limit=60, offset=offset, beforeTime=0, commentId="0",
                    compareUserLocation=False, composeConcert=False, markReplied=False,
                    forceFlatComment=False, showInner=False)
            assert calls[0]["args"] == {"resourceType": 4, "resourceIds": '["1"]'}
            assert calls[-1]["args"] == {"resourceType": 4, "resourceIds": '["2"]'}
            assert rows["embedded_first"]["latest"] == 60 and rows["embedded_first"]["hot"] == 5
            assert rows["embedded_append"]["latest"] == 63 and rows["embedded_append"]["hot"] == 5
            assert rows["return_focus"]["tab"] == "lyrics" and rows["hot_route"]["tab"] is None
            assert rows["zero_count"]["tab"] == "comments" and rows["zero_count"]["trackId"] == "2"
            assert len({row["playId"] for key, row in rows.items() if key != "zero_count"}) == 1
        else:
            calls = rows["control_route"]["vinylCommentsCalls"]
            assert [call["path"] for call in calls] == [COUNT, LATEST]
            assert calls[1]["args"]["limit"] == 20 and rows["control_route"]["tab"] is None
        if not windows:
            locale = read(output / "locale.json")
            assert locale["numeric"] == "C"
            assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                       for key in ("messages", "characters"))
        if args.screenshots:
            assert len(list(output.glob("ux_*.png"))) == len(names)
        summary[group] = list(rows)
    report = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, states=summary,
                  realMusicRequests=0, realAccountWrites=0)
    (root / "audit.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
