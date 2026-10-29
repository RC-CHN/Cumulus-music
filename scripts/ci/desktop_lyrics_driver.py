"""Independent OS keys and native input-region checks for the lyric WebView.

This checks X11 input shapes / Windows extended window styles, not physical
display performance or every compositor's click-through implementation.
"""
import ctypes as C
import ctypes.util
import json
import subprocess
import sys
import time


class Rectangle(C.Structure):
    _fields_ = [('x', C.c_short), ('y', C.c_short), ('width', C.c_ushort), ('height', C.c_ushort)]


class InputRegion:
    def __init__(self, keyboard):
        self.keyboard = keyboard
        if sys.platform == 'win32':
            from ctypes import wintypes as W
            self.user = C.WinDLL('user32', use_last_error=True)
            self.user.FindWindowW.argtypes = [W.LPCWSTR, W.LPCWSTR]
            self.user.FindWindowW.restype = W.HWND
            self.user.GetWindowLongW.argtypes = [W.HWND, C.c_int]
            self.user.GetWindowLongW.restype = C.c_long
        else:
            self.x = keyboard.x
            self.ext = C.CDLL(ctypes.util.find_library('Xext'))
            self.x.XQueryTree.argtypes = [C.c_void_p, C.c_ulong, C.POINTER(C.c_ulong),
                                          C.POINTER(C.c_ulong), C.POINTER(C.POINTER(C.c_ulong)), C.POINTER(C.c_uint)]
            self.x.XQueryTree.restype = C.c_int
            self.x.XFetchName.argtypes = [C.c_void_p, C.c_ulong, C.POINTER(C.c_void_p)]
            self.x.XFetchName.restype = C.c_int
            self.x.XFree.argtypes = [C.c_void_p]
            self.x.XFree.restype = C.c_int
            self.x.XInternAtom.argtypes = [C.c_void_p, C.c_char_p, C.c_int]
            self.x.XInternAtom.restype = C.c_ulong
            self.x.XGetWindowProperty.argtypes = [C.c_void_p, C.c_ulong, C.c_ulong, C.c_long, C.c_long,
                C.c_int, C.c_ulong, C.POINTER(C.c_ulong), C.POINTER(C.c_int),
                C.POINTER(C.c_ulong), C.POINTER(C.c_ulong), C.POINTER(C.c_void_p)]
            self.x.XGetWindowProperty.restype = C.c_int
            self.ext.XShapeGetRectangles.argtypes = [C.c_void_p, C.c_ulong, C.c_int, C.POINTER(C.c_int), C.POINTER(C.c_int)]
            self.ext.XShapeGetRectangles.restype = C.c_void_p

    def read(self):
        title = 'Cumulus · 桌面歌词'
        if sys.platform == 'win32':
            window = self.user.FindWindowW(None, title)
            assert window, 'native lyric window absent'
            style = self.user.GetWindowLongW(window, -20)
            return {'platform': 'windows', 'inputSuppressed': bool(style & 0x20),
                    'layered': bool(style & 0x80000)}
        root, parent, count = C.c_ulong(), C.c_ulong(), C.c_uint()
        children = C.POINTER(C.c_ulong)()
        assert self.x.XQueryTree(self.keyboard.display, self.keyboard.root,
                                C.byref(root), C.byref(parent), C.byref(children), C.byref(count))
        found, names = None, []
        atom = self.x.XInternAtom(self.keyboard.display, b'_NET_WM_NAME', 0)
        try:
            for index in range(count.value):
                name, kind, bits, length, after = C.c_void_p(), C.c_ulong(), C.c_int(), C.c_ulong(), C.c_ulong()
                status = self.x.XGetWindowProperty(self.keyboard.display, children[index], atom, 0, 1024, 0, 0,
                    C.byref(kind), C.byref(bits), C.byref(length), C.byref(after), C.byref(name))
                if status == 0 and name and bits.value == 8:
                    try:
                        text = C.string_at(name, length.value).decode('utf-8', errors='replace')
                        names.append(text)
                        if text == title:
                            found = children[index]
                    finally:
                        self.x.XFree(name)
                elif name:
                    self.x.XFree(name)
        finally:
            if children:
                self.x.XFree(children)
        assert found, f'native lyric X11 window absent: {names}'
        size, ordering = C.c_int(), C.c_int()
        rectangles = self.ext.XShapeGetRectangles(self.keyboard.display, found, 2,
                                                  C.byref(size), C.byref(ordering))
        bounds = []
        if rectangles:
            values = C.cast(rectangles, C.POINTER(Rectangle))
            bounds = [dict(x=values[i].x,y=values[i].y,width=values[i].width,height=values[i].height) for i in range(size.value)]
            self.x.XFree(rectangles)
        area = sum(r['width'] * r['height'] for r in bounds)
        # tao 0.35.3 deliberately retains a 1x1 origin input region on X11.
        return {'platform': 'x11', 'inputSuppressed': area <= 1,
                'fullInputTransparency': area == 0, 'inputArea': area, 'inputRectangles': bounds}


def launch(binary, env, log, output, capture):
    windows = sys.platform == 'win32'
    if windows:
        from shortcut_windows_keys import Keyboard
    else:
        from shortcut_smoke import Keyboard
    keyboard = Keyboard()
    region = InputRegion(keyboard)
    if capture:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
    seen, sent, regions = set(), [], []
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    try:
        deadline = time.monotonic() + 240
        while process.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError('lyric WebView did not finish')
            if windows:
                keyboard.drain()
            for path in sorted(output.glob('key-*.json')):
                reply = path.with_suffix('.done')
                if reply.exists():
                    continue
                value = json.loads(path.read_text(encoding='utf-8'))
                assert value['key'] in ['d', 'Escape', 'Tab'], value
                if value['key'] == 'd':
                    keyboard.chord('d')
                else:
                    # Keep the real main WebView focused for local dialog keys.
                    keyboard.event(value['key'], True)
                    time.sleep(.06)
                    keyboard.event(value['key'], False)
                sent.append(value['key'])
                reply.write_text('sent', encoding='utf-8')
            for path in output.glob('ux_*.json'):
                if path in seen:
                    continue
                value = json.loads(path.read_text(encoding='utf-8'))
                if value['surface'] == 'lyric':
                    native = region.read()
                    assert native['inputSuppressed'] == value['locked'], (path.name, native, value['locked'])
                    regions.append({'stage': path.stem, **native})
                if capture:
                    window = Gdk.get_default_root_window()
                    picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                    assert picture is not None
                    picture.savev(str(path.with_suffix('.png')), 'png', [], [])
                seen.add(path)
            time.sleep(.05)
        (output / 'native-input.json').write_text(json.dumps({'keys': sent, 'regions': regions}, indent=2)+'\n', encoding='utf-8')
        return process.returncode
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        keyboard.close()
