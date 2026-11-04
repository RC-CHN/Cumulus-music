"""Explicitly provide a WM to an isolated Xvfb probe; never replace a user's WM.

Only CUMULUS_TEST_WINDOW_MANAGER=openbox enables this test launcher. Production
startup does not import it. The child uses the probe's isolated XDG profile.
"""
import ctypes
import ctypes.util
import os
from pathlib import Path
import subprocess
import time
import motion_smoke


class ManagerCheck:
    def __init__(self, display):
        self.x11 = ctypes.CDLL(ctypes.util.find_library('X11'))
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
        self.x11.XDefaultRootWindow.restype = ctypes.c_ulong
        self.x11.XInternAtom.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]
        self.x11.XInternAtom.restype = ctypes.c_ulong
        self.x11.XGetWindowProperty.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_long, ctypes.c_long,
                                               ctypes.c_int, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_int),
                                               ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_void_p)]
        self.x11.XFree.argtypes = [ctypes.c_void_p]
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.display = self.x11.XOpenDisplay(display.encode())
        if not self.display:
            raise RuntimeError('explicit WM test requires an X11 display')
        self.root = self.x11.XDefaultRootWindow(self.display)
        self.atom = self.x11.XInternAtom(self.display, b'_NET_SUPPORTING_WM_CHECK', 0)

    def ready(self):
        kind, count, remaining, data, bits = ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_ulong(), ctypes.c_void_p(), ctypes.c_int()
        result = self.x11.XGetWindowProperty(self.display, self.root, self.atom, 0, 1, 0, 33,
                                             ctypes.byref(kind), ctypes.byref(bits), ctypes.byref(count), ctypes.byref(remaining), ctypes.byref(data))
        try:
            return result == 0 and kind.value == 33 and bits.value == 32 and count.value == 1 and bool(data.value) and ctypes.cast(data, ctypes.POINTER(ctypes.c_ulong))[0] != 0
        finally:
            if data.value:
                self.x11.XFree(data)

    def close(self):
        self.x11.XCloseDisplay(self.display)


def launch(binary, env, log, output: Path, capture, *, before_ack=None):
    requested = os.environ.get('CUMULUS_TEST_WINDOW_MANAGER')
    if not requested:
        return motion_smoke.launch(binary, env, log, output, capture, before_ack=before_ack)
    if os.name == 'nt' or requested != 'openbox':
        raise RuntimeError('unsupported explicit test window manager')
    check = ManagerCheck(env.get('DISPLAY', ''))
    manager = None
    try:
        if not check.ready():
            with Path(log.name).with_name('window-manager.log').open('x', encoding='utf-8') as manager_log:
                manager = subprocess.Popen(['openbox', '--sm-disable'], env=env, stdout=manager_log, stderr=manager_log)
                deadline = time.monotonic()+6
                while not check.ready():
                    if manager.poll() is not None or time.monotonic() >= deadline:
                        raise RuntimeError('isolated window manager did not become ready')
                    time.sleep(.05)
        return motion_smoke.launch(binary, env, log, output, capture, before_ack=before_ack)
    finally:
        if manager is not None and manager.poll() is None:
            manager.terminate()
            try:
                manager.wait(timeout=5)
            except subprocess.TimeoutExpired:
                manager.kill()
                manager.wait()
        check.close()
