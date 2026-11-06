"""Real Xvfb drag, Ctrl+C, clipboard readback and lyric seeks in Tauri WebKitGTK."""

import argparse
from contextlib import ExitStack
import ctypes
import json
import os
from pathlib import Path
import re
import sys
import time

from queue_collection_smoke import isolated_profile
from queue_row_collection_smoke import QueueCollectInput
from window_manager import launch


EXPECTED = '合成歌词 4\n合成翻译 4\n合成歌词 5\n'
SENTINEL = 'cumulus sealed lyric clipboard: untouched'
LABELS = ('drag_ready', 'copy_ready', 'copied', 'pointer_seek', 'keyboard_seek', 'space_seek',
          'same_row_drag_ready', 'same_row_dragged')


def require_xvfb():
    display = os.environ.get('DISPLAY', '')
    match = re.fullmatch(r'(:\d+)(?:\.\d+)?', display)
    assert sys.platform == 'linux' and os.environ.get('GDK_BACKEND') == 'x11' and match, \
        'run under a dedicated Xvfb with GDK_BACKEND=x11'
    for process in Path('/proc').glob('[0-9]*/cmdline'):
        try:
            command = process.read_bytes().split(b'\0')
        except OSError:
            continue
        if command and command[0].split(b'/')[-1] == b'Xvfb' and match[1].encode('ascii') in command[1:]:
            return
    raise AssertionError('refusing pointer/clipboard input on a non-Xvfb display')


class TrustedLyricInput(QueueCollectInput):
    def __init__(self):
        super().__init__()
        self.x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
        self.x11.XStringToKeysym.restype = ctypes.c_ulong
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.restype = ctypes.c_ubyte
        self.xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
        self.xtest.XTestFakeKeyEvent.restype = ctypes.c_int
        import gi
        gi.require_version('Gdk', '3.0')
        gi.require_version('Gtk', '3.0')
        from gi.repository import Gdk, GLib, Gtk
        self.glib = GLib
        self.clipboard = Gtk.Clipboard.get(Gdk.SELECTION_CLIPBOARD)
        self.html_target = Gdk.Atom.intern('text/html', False)
        self.copied = None
        self.rich_html_present = None

    def clipboard_request(self, register, project):
        loop = self.glib.MainLoop()
        result, timed_out = [], []

        def timeout():
            timed_out.append(True)
            loop.quit()
            return False

        def received(_clipboard, data, _user_data):
            result.append(project(data))
            loop.quit()

        timer = self.glib.timeout_add(3000, timeout)
        try:
            register(received)
            if not result:
                loop.run()
        finally:
            if not timed_out:
                self.glib.source_remove(timer)
        assert not timed_out and len(result) == 1, 'GTK clipboard readback timed out'
        return result[0]

    def read_clipboard(self):
        return self.clipboard_request(lambda callback: self.clipboard.request_text(callback, None),
                                      lambda text: text)

    def clipboard_has_html(self):
        return self.clipboard_request(
            lambda callback: self.clipboard.request_contents(self.html_target, callback, None),
            lambda selection: selection.get_length() >= 0)

    def key_event(self, name, pressed):
        key = self.x11.XKeysymToKeycode(self.display, self.x11.XStringToKeysym(name.encode('ascii')))
        assert key and self.xtest.XTestFakeKeyEvent(self.display, key, int(pressed), 0), name
        self.x11.XSync(self.display, 0)

    def key(self, *keys):
        pressed = []
        try:
            for name in keys:
                self.key_event(name, True)
                pressed.append(name)
        finally:
            for name in reversed(pressed):
                self.key_event(name, False)

    def at_capture(self, path, payload, pid):
        label = path.stem.split('_', 2)[2]
        if label in ('drag_ready', 'same_row_drag_ready'):
            if label == 'drag_ready':
                self.clipboard.set_text(SENTINEL, -1)
                assert self.read_clipboard() == SENTINEL
            start, end = (self.pointer_target(payload['targets'][key], pid) for key in ('from', 'to'))
            assert start['clientX'] == end['clientX']
            if label == 'drag_ready':
                assert end['y'] - start['y'] > 15, (start, end)
            else:
                assert end['y'] == start['y'] and end['x'] - start['x'] > 15, (start, end)
            path.with_name(f'{label}-trusted-input.json').write_text(
                json.dumps({'from': start, 'to': end}, indent=2) + '\n', encoding='utf-8')
            self.move(start)
            self.button(True)
            try:
                # Send a physical path through the intervening text, not a
                # scripted Selection object or a single coalesced jump.
                time.sleep(.06)
                for step in range(1, 13):
                    self.move({axis: round(start[axis] + (end[axis] - start[axis]) * step / 12)
                               for axis in ('x', 'y')})
                    time.sleep(.02)
            finally:
                self.button(False)
        elif label == 'copy_ready':
            assert self.read_clipboard() == SENTINEL, 'drag alone changed the clipboard'
            self.key('Control_L', 'c')
        elif label == 'copied':
            self.copied = self.read_clipboard()
            self.rich_html_present = self.clipboard_has_html()
            path.with_name('clipboard-readback.json').write_text(
                json.dumps({'clipboard': self.copied, 'richHtmlPresent': self.rich_html_present},
                           ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
            assert self.copied == EXPECTED, repr(self.copied)
            assert self.rich_html_present is False, 'lyric copy must not publish text/html'
            point = self.pointer_target(payload['target'], pid)
            self.move(point)
            self.button(True)
            self.button(False)
        elif label == 'pointer_seek':
            assert payload['focused'] == '4'
            self.key('Return')
        elif label == 'keyboard_seek':
            assert payload['focused'] == '5'
            self.key('space')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/vinyl_lyric_copy_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_PLAYER_LAYOUT_PROOF='1',
               CUMULUS_QUEUE_UI_PROOF='1', CUMULUS_UX_SCRIPT=str(source),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    isolated_profile(env, root)
    output = root / 'first'
    with ExitStack() as stack:
        driver = TrustedLyricInput()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        status = launch(args.binary.resolve(strict=True), env, log, output, True, before_ack=driver.at_capture)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exited {status}'
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(LABELS)
    assert complete['realMusicRequests'] == complete['accountWrites'] == 0
    assert len(list(output.glob('ux_*.json'))) == len(list(output.glob('ux_*.png'))) == len(LABELS)
    stages = [read(output / f'ux_{index:02d}_{label}.json') for index, label in enumerate(LABELS, 1)]
    baseline = stages[0]['nativePlayer']
    assert baseline['track']['id'] == '1' and baseline['playId'] and baseline['entryId']
    positions = []
    for sample, seconds in zip(stages, (4.25, 4.25, 4.25, 5, 4, 5, 5, 5), strict=True):
        actual = sample['nativePlayer']
        assert actual['state'] == 'paused' and not actual['nativeSeeking'] and not actual['error']
        assert actual['track']['id'] == '1' and sample['nativeQueue'] == stages[0]['nativeQueue']
        assert all(actual[key] == baseline[key] for key in ('playId', 'entryId', 'queueRevision'))
        assert abs(actual['position'] - seconds) < .12
        positions.append(actual['position'])
    assert positions[0] == positions[1] == positions[2], positions
    assert positions[5] == positions[6] == positions[7], positions
    assert stages[2]['copy'] == {'trusted': True, 'handled': True, 'target': '4', 'hasClipboardData': True}
    assert stages[1]['selection']['start'] == '4' and stages[1]['selection']['end'] == '5'
    assert stages[-1]['selection'] == {'text': '合成歌词 4', 'start': '4', 'end': '4', 'collapsed': False}
    assert driver.copied == EXPECTED
    report = {'ok': True, 'actualWebView': True, 'trustedX11': True, 'clipboard': driver.copied,
              'richHtmlPresent': driver.rich_html_present,
              'nativePositions': positions, 'nativePlayIdUnchanged': True, 'nativeQueueUnchanged': True,
              'screenshots': len(LABELS), 'stages': LABELS, 'realMusicRequests': 0, 'accountWrites': 0,
              'physicalAudioOutputVerified': False}
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
