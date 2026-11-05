"""Verify the production desktop vinyl modes with the existing sealed audio fixture."""

import argparse
import ctypes
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def x_key(key):
    x11 = ctypes.CDLL("libX11.so.6")
    xtst = ctypes.CDLL("libXtst.so.6")
    display_type = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = display_type
    x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
    x11.XStringToKeysym.restype = ctypes.c_ulong
    x11.XKeysymToKeycode.argtypes = [display_type, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    x11.XFlush.argtypes = [display_type]
    x11.XCloseDisplay.argtypes = [display_type]
    xtst.XTestFakeKeyEvent.argtypes = [display_type, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    display = x11.XOpenDisplay(None)
    assert display
    try:
        code = x11.XKeysymToKeycode(display, x11.XStringToKeysym(key.encode("ascii")))
        assert code and xtst.XTestFakeKeyEvent(display, code, 1, 0)
        assert xtst.XTestFakeKeyEvent(display, code, 0, 0)
        x11.XFlush(display)
    finally:
        x11.XCloseDisplay(display)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    parser.add_argument("--native-keys", action="store_true", help="Linux X11 screenshot run only")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if (sys.platform not in {"linux", "win32"} or windows != bool(args.runtime)
            or (windows and args.screenshots) or (args.native_keys and not args.screenshots)):
        parser.error("Windows needs --runtime; Linux X11 screenshots are required for --native-keys")
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
    for key in profiles:
        isolated = root / "profile" / key
        isolated.mkdir(parents=True, mode=0o700)
        env[key] = str(isolated)
    if args.frontend_dist:
        env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF="1")
    source = (Path(__file__).resolve().parents[2] / "desktop/examples/vinyl_display_ui_probe.js").read_text(encoding="utf-8")
    expected = {"first": {
        "standard": "classic", "cover_sheet": "cover", "black_vinyl": "cover",
        "centered_lyric": "lyric",
    }, "reopen": {"fm_centered": "lyric", "fm_cover": "cover", "fm_reopen": "cover"}}
    result = {}
    for phase, names in expected.items():
        reopened = phase == "reopen"
        script = root / f"{phase}-probe.js"
        script.write_text(f"window.__vinylReopened={json.dumps(reopened)};\n"
                          f"window.__vinylNativeKeys={json.dumps(args.native_keys)};\n" + source,
                          encoding="utf-8")
        run_env = {**env, "CUMULUS_UX_SCRIPT": str(script)}
        if reopened:
            run_env.update(CUMULUS_PROOF_REOPEN="1", CUMULUS_VERTICAL_ZONE_PROOF="fm-preview")

        def keyboard(path, _payload, _pid):
            if path.stem.endswith("_standard"):
                x_key("Right")
            elif path.stem.endswith("_cover_sheet"):
                x_key("Escape")

        output = root / phase
        with (root / f"{phase}.log").open("x", encoding="utf-8") as log:
            code = launch(binary, run_env, log, output, args.screenshots,
                          before_ack=keyboard if args.native_keys else None)
        failed = output / "failed.json"
        assert not failed.exists(), read(failed) if failed.exists() else None
        assert code == 0, (phase, code)
        complete = read(output / "complete.json")
        assert complete["ok"] and complete["stageCount"] == len(names) and complete["reopened"] == reopened
        rows = {path.stem.split("_", 2)[2]: read(path) for path in sorted(output.glob("ux_*.json"))}
        assert {key: row["mode"] for key, row in rows.items()} == names
        for key, row in rows.items():
            native = row["nativePlayer"]
            assert native["state"] == "paused" and native["fm"] == key.startswith("fm_")
            assert native["playId"] == row["playId"] and native["queueRevision"] == row["queueRevision"]
            assert abs(native["position"] - row["position"]) < .1 and row["audioFactoryCount"] == 1
        assert len({row["playId"] for row in rows.values()}) == 1
        assert len({row["queueRevision"] for row in rows.values()}) == 1
        if not reopened:
            assert rows["black_vinyl"]["position"] > rows["standard"]["position"] + .5
        else:
            # Once the zone actor is paused, changing or reopening layouts must
            # not repeat its recommendation, song-detail, or URL authorization.
            calls = rows["fm_centered"]["zoneFmPreview"]["calls"]
            assert all(row["zoneFmPreview"]["calls"] == calls for row in rows.values())
        if args.screenshots:
            assert len(list(output.glob("ux_*.png"))) == len(names)
        if not windows and reopened:
            locale = read(output / "locale.json")
            assert locale["numeric"] == "C"
            assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                       for key in ("messages", "characters"))
        result[phase] = list(rows)
    report = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, nativeKeys=args.native_keys,
                  processRestart=True, states=result, realMusicRequests=0, realAccountWrites=0)
    (root / "audit.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
