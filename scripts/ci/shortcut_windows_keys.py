"""Independent native Windows input producer for the synthetic shortcut host.

This process owns its own window and an intentionally conflicting hotkey. It
uses SendInput, never invokes Cumulus business actions to emulate a key press.
"""
import ctypes as C
from ctypes import wintypes as W
import time


class Mouse(C.Structure):
    _fields_ = [('dx', W.LONG), ('dy', W.LONG), ('data', W.DWORD), ('flags', W.DWORD), ('time', W.DWORD), ('extra', C.c_size_t)]


class Key(C.Structure):
    _fields_ = [('vk', W.WORD), ('scan', W.WORD), ('flags', W.DWORD), ('time', W.DWORD), ('extra', C.c_size_t)]


class Hardware(C.Structure):
    _fields_ = [('message', W.DWORD), ('low', W.WORD), ('high', W.WORD)]


class Union(C.Union):
    _fields_ = [('mouse', Mouse), ('key', Key), ('hardware', Hardware)]


class Input(C.Structure):
    _fields_ = [('kind', W.DWORD), ('value', Union)]


class Keyboard:
    def __init__(self):
        self.u = C.WinDLL('user32', use_last_error=True)
        for name, result, args in [
            ('CreateWindowExW', W.HWND, [W.DWORD, W.LPCWSTR, W.LPCWSTR, W.DWORD, C.c_int, C.c_int, C.c_int, C.c_int, W.HWND, W.HMENU, W.HINSTANCE, W.LPVOID]),
            ('SetForegroundWindow', W.BOOL, [W.HWND]),
            ('GetForegroundWindow', W.HWND, []),
            ('SetFocus', W.HWND, [W.HWND]),
            ('RegisterHotKey', W.BOOL, [W.HWND, C.c_int, W.UINT, W.UINT]),
            ('UnregisterHotKey', W.BOOL, [W.HWND, C.c_int]),
            ('SendInput', W.UINT, [W.UINT, C.POINTER(Input), C.c_int]),
            ('PeekMessageW', W.BOOL, [C.POINTER(W.MSG), W.HWND, W.UINT, W.UINT, W.UINT]),
            ('TranslateMessage', W.BOOL, [C.POINTER(W.MSG)]),
            ('DispatchMessageW', C.c_ssize_t, [C.POINTER(W.MSG)]),
            ('DestroyWindow', W.BOOL, [W.HWND]),
        ]:
            fn = getattr(self.u, name)
            fn.restype, fn.argtypes = result, args
        self.window = self.u.CreateWindowExW(0, 'STATIC', 'Cumulus independent shortcut producer', 0x10CF0000, 10, 10, 320, 140, None, None, None, None)
        assert self.window, C.get_last_error()
        self.reserved = False
        self.drain()

    def drain(self):
        message = W.MSG()
        for _ in range(100):
            if not self.u.PeekMessageW(C.byref(message), None, 0, 0, 1): break
            self.u.TranslateMessage(C.byref(message))
            self.u.DispatchMessageW(C.byref(message))

    def event(self, name, down):
        keys = {'Control_L': 17, 'Alt_L': 18, 'Shift_L': 16, 'Left': 37, 'Up': 38, 'Right': 39, 'Down': 40, 'XF86AudioPlay': 179, 'Escape': 27, 'Tab': 9}
        key = keys.get(name, ord(name.upper()) if len(name) == 1 else None)
        assert key is not None, name
        flags = (0 if down else 2) | (1 if key in [37, 38, 39, 40, 179] else 0)
        value = Input(kind=1, value=Union(key=Key(vk=key, flags=flags)))
        assert self.u.SendInput(1, C.byref(value), C.sizeof(value)) == 1, ('SendInput', C.get_last_error())
        self.drain()

    def focus(self):
        self.drain()
        self.u.SetForegroundWindow(self.window)
        self.u.SetFocus(self.window)
        self.drain()
        assert self.u.GetForegroundWindow() == self.window, 'independent producer did not obtain foreground window'

    def chord(self, key, hold=.06, modifiers=('Control_L', 'Alt_L')):
        self.focus()
        keys = [*modifiers, key]
        try:
            for k in keys: self.event(k, True)
            time.sleep(hold)
        finally:
            for k in reversed(keys): self.event(k, False)

    def conflict(self, enabled):
        if enabled:
            assert self.u.RegisterHotKey(None, 0x123, 1 | 2 | 0x4000, 80), C.get_last_error()
        else:
            assert self.u.UnregisterHotKey(None, 0x123), C.get_last_error()
        self.reserved = enabled

    def close(self):
        if self.reserved: self.conflict(False)
        self.u.DestroyWindow(self.window)
