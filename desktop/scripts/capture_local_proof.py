"""Xvfb-only local UI capture and native GTK folder selection (test host)."""
import ctypes
import json
import os
import pathlib
import re
import subprocess
import time
import gi

gi.require_version("Gdk", "3.0")
from gi.repository import Gdk

root = pathlib.Path(os.environ["CUMULUS_PROOF_DIR"])
reopened = "CUMULUS_PROOF_REOPEN" in os.environ
suffix = "-reopened" if reopened else ""


def select_folder(path):
    x11 = ctypes.CDLL("libX11.so.6")
    xtest = ctypes.CDLL("libXtst.so.6")
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
    x11.XStringToKeysym.restype = ctypes.c_ulong
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_uint
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
    x11.XRaiseWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtest.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    display = x11.XOpenDisplay(None)
    if not display:
        raise RuntimeError("Xvfb display unavailable")

    def key(name, pressed):
        code = x11.XKeysymToKeycode(display, x11.XStringToKeysym(name.encode("ascii")))
        if not code:
            raise RuntimeError("unsupported fixture path key")
        xtest.XTestFakeKeyEvent(display, code, int(pressed), 0)
        x11.XFlush(display)

    def tap(name):
        key(name, True)
        key(name, False)

    try:
        windows = subprocess.check_output(["xwininfo", "-root", "-tree"], text=True)
        match = re.search(r'(0x[0-9a-f]+) "选择本地音乐目录"', windows)
        if not match:
            raise RuntimeError("native chooser window disappeared")
        chooser = int(match[1], 16)
        x11.XRaiseWindow(display, chooser)
        x11.XSetInputFocus(display, chooser, 2, 0)
        x11.XFlush(display)
        time.sleep(1)
        key("Control_L", True)
        tap("l")
        key("Control_L", False)
        time.sleep(0.3)
        key("Control_L", True)
        tap("a")
        key("Control_L", False)
        for char in str(path):
            shifted = char.isupper() or char == "_"
            if shifted:
                key("Shift_L", True)
            tap({"/": "slash", ".": "period", "-": "minus", "_": "underscore"}.get(char, char))
            if shifted:
                key("Shift_L", False)
            time.sleep(0.01)
        tap("Return")
        time.sleep(1.5)
        window = Gdk.get_default_root_window()
        capture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
        capture.savev(str(root / "picker-location.png"), "png", [], [])
        geometry = subprocess.check_output(["xwininfo", "-id", hex(chooser)], text=True)
        def number(label):
            return int(re.search(re.escape(label) + r"\s+(-?\d+)", geometry)[1])
        x = number("Absolute upper-left X:") + number("Width:") - 45
        y = number("Absolute upper-left Y:") + number("Height:") - 24
        xtest.XTestFakeMotionEvent(display, -1, x, y, 0)
        xtest.XTestFakeButtonEvent(display, 1, 1, 0)
        xtest.XTestFakeButtonEvent(display, 1, 0, 0)
        x11.XFlush(display)
    finally:
        x11.XCloseDisplay(display)


seen = set()
with (root.parent / (root.name + suffix + "-host.log")).open("w") as log:
    process = subprocess.Popen(["dbus-run-session", "--", ".build/target/debug/examples/local_ui_probe"], stdout=log, stderr=log)
    deadline = time.monotonic() + 300
    try:
        while process.poll() is None and time.monotonic() < deadline:
            for path in root.glob(f"local_*{suffix}.json"):
                if ("-reopened" in path.stem) != reopened or path.stem in seen:
                    continue
                if path.stem == "local_picker":
                    dialog_deadline = time.monotonic() + 20
                    while time.monotonic() < dialog_deadline:
                        windows = subprocess.check_output(["xwininfo", "-root", "-tree"], text=True)
                        if "选择本地音乐目录" in windows:
                            break
                        time.sleep(0.1)
                    else:
                        raise RuntimeError("native directory dialog was not observed")
                    time.sleep(0.5)
                window = Gdk.get_default_root_window()
                capture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                capture.savev(str(path.with_suffix(".png")), "png", [], [])
                seen.add(path.stem)
                print(json.dumps({"captured": path.stem}), flush=True)
                if path.stem == "local_picker":
                    select_folder(root / "media")
            time.sleep(0.1)
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
            raise RuntimeError("local UI proof exceeded its deadline")
        print(json.dumps({"exit_code": process.returncode, "screenshots": len(seen)}), flush=True)
        if process.returncode or not (root / f"complete{suffix}.json").exists():
            raise SystemExit(1)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
