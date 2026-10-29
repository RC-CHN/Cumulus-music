"""Independent OS key producer -> real Tauri -> Rust core -> libmpv/null.

Run under GDK_BACKEND=x11 LC_ALL=en_US.UTF-8 dbus-run-session -- xvfb-run -a.
This proves X11 keys, not Wayland, physical audio, or frontend shortcut editing.
"""
import argparse
import ctypes as C
import ctypes.util
import json
import os
from pathlib import Path
import subprocess
import sys
import time


class Keyboard:
    def __init__(self):
        self.x = C.CDLL(ctypes.util.find_library('X11'))
        self.xt = C.CDLL(ctypes.util.find_library('Xtst'))
        for name, result, args in [
            ('XOpenDisplay', C.c_void_p, [C.c_char_p]),
            ('XDefaultRootWindow', C.c_ulong, [C.c_void_p]),
            ('XStringToKeysym', C.c_ulong, [C.c_char_p]),
            ('XKeysymToKeycode', C.c_ubyte, [C.c_void_p, C.c_ulong]),
            ('XCreateSimpleWindow', C.c_ulong, [C.c_void_p, C.c_ulong, C.c_int, C.c_int, C.c_uint, C.c_uint, C.c_uint, C.c_ulong, C.c_ulong]),
            ('XMapWindow', C.c_int, [C.c_void_p, C.c_ulong]),
            ('XSetInputFocus', C.c_int, [C.c_void_p, C.c_ulong, C.c_int, C.c_ulong]),
            ('XGetInputFocus', C.c_int, [C.c_void_p, C.POINTER(C.c_ulong), C.POINTER(C.c_int)]),
            ('XGrabKey', C.c_int, [C.c_void_p, C.c_int, C.c_uint, C.c_ulong, C.c_int, C.c_int, C.c_int]),
            ('XUngrabKey', C.c_int, [C.c_void_p, C.c_int, C.c_uint, C.c_ulong]),
            ('XSync', C.c_int, [C.c_void_p, C.c_int]),
            ('XCloseDisplay', C.c_int, [C.c_void_p]),
        ]:
            fn = getattr(self.x, name)
            fn.restype, fn.argtypes = result, args
        self.xt.XTestFakeKeyEvent.argtypes = [C.c_void_p, C.c_uint, C.c_int, C.c_ulong]
        self.xt.XTestFakeKeyEvent.restype = C.c_int
        self.display = self.x.XOpenDisplay(None)
        assert self.display, 'X11 display unavailable'
        self.root = self.x.XDefaultRootWindow(self.display)
        self.window = self.x.XCreateSimpleWindow(self.display, self.root, 10, 10, 200, 100, 0, 0, 0)
        self.x.XMapWindow(self.display, self.window)
        self.x.XSync(self.display, 0)

    def code(self, name):
        value = self.x.XKeysymToKeycode(self.display, self.x.XStringToKeysym(name.encode()))
        assert value, 'keysym absent: ' + name
        return value

    def focus(self):
        self.x.XSetInputFocus(self.display, self.window, 1, 0)
        self.x.XSync(self.display, 0)
        window, revert = C.c_ulong(), C.c_int()
        self.x.XGetInputFocus(self.display, C.byref(window), C.byref(revert))
        assert window.value == self.window, 'independent producer window did not own focus'

    def event(self, key, down):
        assert self.xt.XTestFakeKeyEvent(self.display, self.code(key), int(down), 0)
        self.x.XSync(self.display, 0)

    def chord(self, key, hold=.06, modifiers=('Control_L', 'Alt_L')):
        self.focus()
        keys = [*modifiers, key]
        try:
            for k in keys: self.event(k, True)
            time.sleep(hold)
        finally:
            for k in reversed(keys): self.event(k, False)

    def conflict(self, enabled):
        # Start before the host. Failure to acquire causes Xlib's default error
        # handler to abort the proof; it cannot be mistaken for a conflict test.
        for lock in [0, 2, 16, 18]:
            if enabled:
                self.x.XGrabKey(self.display, self.code('p'), 4 | 8 | lock, self.root, 0, 1, 1)
            else:
                self.x.XUngrabKey(self.display, self.code('p'), 4 | 8 | lock, self.root)
        self.x.XSync(self.display, 0)

    def close(self):
        self.x.XCloseDisplay(self.display)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    windows = sys.platform == 'win32'
    if not windows:
        assert not env.get('WAYLAND_DISPLAY') and env.get('XDG_SESSION_TYPE') != 'wayland', 'requires a real X11 test session'
    env['CUMULUS_PROOF_DIR'] = str(run)
    for key, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / directory
        path.mkdir(mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if windows:
        from shortcut_windows_keys import Keyboard as WindowsKeyboard
        keyboard = WindowsKeyboard()
    else:
        keyboard = Keyboard()
    process, sequence, steps = None, 0, []

    def state(): return json.loads((run / 'state.json').read_text(encoding='utf-8'))

    def wait(condition, message, seconds=20):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if windows: keyboard.drain()
            result = condition()
            if result: return result
            if process.poll() is not None: raise AssertionError(f'host exited {process.returncode}: {message}')
            time.sleep(.025)
        raise AssertionError('timed out: ' + message)

    def control(kind, expected=True, **args):
        nonlocal sequence
        sequence += 1
        pending = run / 'command.pending'
        pending.write_text(json.dumps({'kind': kind, 'id': sequence, **args}), encoding='utf-8')
        pending.replace(run / 'command.json')
        reply = run / f'ack-{sequence}.json'
        wait(reply.exists, kind)
        assert json.loads(reply.read_text(encoding='utf-8'))['ok'] == expected, kind

    def ready():
        wait(lambda: state()['shortcuts']['phase'] == 'ready', 'OS registration acknowledgement')

    def edit(value, expected=True):
        revision = state()['shortcutSettings']['current']['revision']
        control('shortcut_change', expected, command={'kind': 'change', 'expected_revision': revision, 'edit': value})
        if expected:
            wait(lambda: state()['shortcutSettings']['current']['revision'] != revision, 'committed settings')
            ready()

    def binding(action, scope='global'):
        return next((r for r in state()['shortcuts']['rows'] if r['scope'] == scope and r['hotkey'] == {'kind': 'configured', 'action': action}), None)

    def expect_state(status): wait(lambda: state()['status'] == status, 'native ' + status)

    def stable(status):
        deadline = time.monotonic() + .5
        while time.monotonic() < deadline:
            assert state()['status'] == status, state()
            time.sleep(.04)

    def record(label): steps.append({'label': label, 'state': state()})

    try:
        keyboard.conflict(True)
        with (root / 'host.log').open('x') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
            wait(lambda: (run / 'ready.json').exists() and (run / 'state.json').exists(), 'native GUI')
            ready()
            assert binding('play_pause')['status']['kind'] == 'conflict'
            assert binding('next')['status']['kind'] == 'active'
            assert state()['engines'] == 0
            record('system-conflict-and-lazy-engine')
            keyboard.conflict(False)
            control('shortcut_read')
            wait(lambda: binding('play_pause')['status']['kind'] == 'active', 'conflict released')
            record('explicit-registration-retry')
            control('start')
            expect_state('Playing')
            control('hide')
            keyboard.chord('p', hold=1.2)
            expect_state('Paused')
            stable('Paused')
            assert not state()['visible']
            record('background-toggle-held-repeat-suppressed')
            keyboard.chord('p')
            expect_state('Playing')
            keyboard.chord('p')
            expect_state('Paused')
            record('release-allows-another-press')
            keyboard.focus()
            for k in ['Control_L', 'Alt_L', 'p']: keyboard.event(k, True)
            expect_state('Playing')
            control('shortcut_read')
            ready()
            time.sleep(.8)
            stable('Playing')
            for k in ['p', 'Alt_L', 'Control_L']: keyboard.event(k, False)
            time.sleep(.15)
            keyboard.chord('p')
            expect_state('Paused')
            record('held-key-replacement-waits-for-release')
            for k in ['Control_L', 'Alt_L', 'p']: keyboard.event(k, True)
            expect_state('Playing')
            control('shortcut_read')
            ready()
            # Release before X11 auto-repeat starts: a new passive grab may not
            # receive this release because it never observed the original press.
            for k in ['p', 'Alt_L', 'Control_L']: keyboard.event(k, False)
            time.sleep(.15)
            keyboard.chord('p')
            expect_state('Paused')
            record('early-release-after-replacement-does-not-swallow-next-press')
            keyboard.chord('Right')
            expect_state('Playing')
            wait(lambda: state()['title'].endswith('合成歌曲 3'), 'official next from pause plays')
            keyboard.chord('Left')
            wait(lambda: state()['title'].endswith('合成歌曲 2'), 'previous track')
            record('official-manual-next-previous')
            keyboard.chord('Down', hold=1.2)
            wait(lambda: state()['volume'] == 95, 'five percent volume step')
            keyboard.chord('Up')
            wait(lambda: state()['volume'] == 100, 'volume clamp')
            record('five-percent-volume-and-repeat')
            control('shortcut_record')
            wait(lambda: state()['shortcuts']['phase'] == 'recording', 'recording releases OS grabs')
            first_recording = state()['shortcuts']['recording']
            keyboard.conflict(True)
            keyboard.conflict(False)
            keyboard.chord('p')
            keyboard.chord('XF86AudioPlay', modifiers=())
            stable('Playing')
            record('recording-releases-global-and-media-grabs')
            control('shortcut_record_end', recording=first_recording)
            ready()
            keyboard.chord('p')
            expect_state('Paused')
            record('recording-cancel-restores-os-delivery')
            control('shortcut_record')
            wait(lambda: state()['shortcuts']['phase'] == 'recording', 'second recording')
            current_recording = state()['shortcuts']['recording']
            assert current_recording != first_recording
            control('shortcut_record_end', recording=first_recording)
            assert state()['shortcuts']['phase'] == 'recording'
            keyboard.chord('p')
            stable('Paused')
            control('shortcut_record_end', recording=current_recording)
            ready()
            keyboard.chord('p')
            expect_state('Playing')
            record('late-cleanup-cannot-release-a-new-recording')
            edit({'kind': 'bind', 'action': 'play_pause', 'scope': 'global', 'keys': [17, 18, 79]})
            keyboard.chord('p')
            stable('Playing')
            keyboard.chord('o')
            expect_state('Paused')
            record('rebind-releases-old-key')
            revision = state()['shortcutSettings']['current']['revision']
            edit({'kind': 'bind', 'action': 'next', 'scope': 'global', 'keys': [17, 18, 79]}, expected=False)
            assert state()['shortcutSettings']['current']['revision'] == revision
            record('logical-conflict-is-atomic')
            control('shortcut_reject')
            edit({'kind': 'global', 'enabled': False}, expected=False)
            assert state()['shortcutSettings']['current']['revision'] == revision
            keyboard.chord('o')
            expect_state('Playing')
            control('shortcut_allow')
            record('failed-save-preserves-active-registration')
            edit({'kind': 'global', 'enabled': False})
            assert binding('play_pause') is None
            keyboard.chord('o')
            stable('Playing')
            keyboard.chord('XF86AudioPlay', modifiers=())
            expect_state('Paused')
            record('global-switch-independent-of-media-keys')
            edit({'kind': 'media', 'enabled': False})
            keyboard.chord('XF86AudioPlay', modifiers=())
            stable('Paused')
            record('media-switch-releases-grab')
            edit({'kind': 'reset'})
            keyboard.chord('p')
            expect_state('Playing')
            control('shortcut_damage')
            wait(lambda: state()['shortcuts']['phase'] == 'damaged', 'damaged settings retire callbacks')
            keyboard.chord('p')
            stable('Playing')
            record('damaged-json-disables-grabs')
            control('shortcut_change', command={'kind': 'repair'})
            ready()
            keyboard.chord('p')
            expect_state('Paused')
            record('explicit-repair-restores-registration')
            control('switch')
            keyboard.chord('p')
            stable('Stopped')
            record('account-switch-has-no-stale-target')
            control('start')
            expect_state('Playing')
            keyboard.chord('p')
            expect_state('Paused')
            record('new-account-uses-device-preferences')
            control('exit')
            assert process.wait(timeout=10) == 0
            keyboard.conflict(True)
            keyboard.conflict(False)
            steps.append({'label': 'process-exit-releases-system-keys'})
            (root / 'report.json').write_text(json.dumps({'passed': True, 'steps': steps, 'backend': 'RegisterHotKey' if windows else 'X11', 'realGui': True, 'realOsKeyEvents': True, 'nullAudio': True, 'physicalHardware': False, 'frontendEditorVerified': False, 'accountRequests': 0}, ensure_ascii=False, indent=2), encoding='utf-8')
    except Exception as error:
        (root / 'failure.json').write_text(json.dumps({'error': str(error), 'steps': steps, 'state': state() if (run / 'state.json').exists() else None}, ensure_ascii=False, indent=2), encoding='utf-8')
        raise
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait()
        keyboard.close()


if __name__ == '__main__': main()
