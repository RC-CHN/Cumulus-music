"""Actual queue row collection, with optional trusted Linux X11 mouse/keyboard input."""

import argparse
from contextlib import ExitStack
import ctypes
import json
import math
import os
from pathlib import Path
import re
import subprocess

from motion_smoke import launch
from queue_collection_smoke import isolated_profile
from search_queue_drop_smoke import TrustedDrag


LABELS = ('pointer_ready', 'pointer_picker', 'pointer_cancelled', 'keyboard_ready',
          'keyboard_picker', 'saved', 'voice_excluded')


class QueueCollectInput(TrustedDrag):
    @staticmethod
    def x11_tool(*argv):
        return subprocess.check_output(argv, text=True, env={**os.environ, 'LC_ALL': 'C'})

    def pointer_target(self, point, pid):
        ids = re.findall(r'0x[0-9a-f]+', self.x11_tool('xprop', '-root', '_NET_CLIENT_LIST'))
        windows = [xid for xid in ids if self.x11_tool('xprop', '-id', xid, '_NET_WM_PID').strip()
                   == f'_NET_WM_PID(CARDINAL) = {pid}']
        assert len(windows) == 1, 'one native X11 client belongs to the probe process'
        info = self.x11_tool('xwininfo', '-id', windows[0])

        def geometry(name):
            match = re.search(r'^\s*' + re.escape(name) + r':\s*(-?\d+)\s*$', info, re.MULTILINE)
            assert match, (name, info)
            return int(match.group(1))

        x, y = geometry('Absolute upper-left X'), geometry('Absolute upper-left Y')
        width, height = geometry('Width'), geometry('Height')
        ratio = point['ratio']
        assert abs(point['width'] * ratio - width) <= 1
        assert abs(point['height'] * ratio - height) <= 1
        # WebKitGTK screenX can report the frame origin plus a window-manager
        # extent even for this undecorated client; use the real client geometry.
        return {'x': x + math.floor(point['x'] * ratio), 'y': y + math.floor(point['y'] * ratio),
                'clientX': x, 'clientY': y, 'clientWidth': width, 'clientHeight': height}

    def at_capture(self, path, payload, pid):
        if path.stem == 'ux_01_pointer_ready':
            point = self.pointer_target(payload['pointer'], pid)
            path.with_name('trusted-pointer-native.json').write_text(
                json.dumps(point, indent=2) + '\n', encoding='utf-8')
            self.move(point)
            self.button(True)
            self.button(False)
        elif path.stem == 'ux_04_keyboard_ready':
            self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
            self.x11.XKeysymToKeycode.restype = ctypes.c_uint8
            self.xtest.XTestFakeKeyEvent.argtypes = [
                ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong,
            ]
            keycode = self.x11.XKeysymToKeycode(self.display, 0xff0d)  # XK_Return
            assert keycode, 'Return is mapped on the isolated X11 display'
            assert self.xtest.XTestFakeKeyEvent(self.display, keycode, 1, 0)
            assert self.xtest.XTestFakeKeyEvent(self.display, keycode, 0, 0)
            self.x11.XSync(self.display, 0)


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def run(args):
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_MODE_UI_PROOF='1',
               CUMULUS_QUEUE_UI_PROOF='1', CUMULUS_PLAYLIST_COLLECT_PROOF='1',
               CUMULUS_QUEUE_COLLECTION_UI_PROOF='1', CUMULUS_QUEUE_ROW_COLLECTION_PROOF='1')
    isolated_profile(env, root)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/queue_row_collection_ui_probe.js'
    script = root / source.name
    script.write_text('window.__queueRowCollectionTrusted = '
                      + json.dumps(args.trusted_input) + ';\n'
                      + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    output = root / 'first'
    with ExitStack() as stack:
        hook = None
        if args.trusted_input:
            driver = QueueCollectInput()
            stack.callback(driver.close)
            hook = driver.at_capture
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots,
                        before_ack=hook)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'inline queue collection exited {status}'
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(LABELS)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == complete['bulkReads'] == 0
    gestures = complete['gestures']
    assert [gesture['kind'] for gesture in gestures] == ['pointer', 'keyboard']
    assert all(gesture['trusted'] == args.trusted_input for gesture in gestures)
    assert gestures[0]['detail'] > 0 and gestures[1]['detail'] == 0
    assert len(list(output.glob('ux_*.json'))) == len(LABELS)
    rows = {}
    for index, label in enumerate(LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        rows[label] = read(path)
        if args.screenshots:
            assert path.with_suffix('.png').is_file()

    baseline = rows['pointer_ready']
    queue, player = baseline['nativeQueue'], baseline['nativePlayer']
    entries = queue['tracks']
    assert [entry['track']['id'] for entry in entries] == ['1', '2', '3', '4']
    assert all(entry['collectible'] is True for entry in entries)
    assert player['state'] == 'paused' and player['track']['id'] == '1' and player['playId']
    assert abs(player['position'] - 10) < .05
    for label in LABELS[:-1]:
        sample = rows[label]
        current = sample['nativePlayer']
        assert sample['nativeQueue'] == queue, label
        assert current['state'] == 'paused' and not current['error'], label
        assert (current['playId'], current['entryId'], current['position']) == (
            player['playId'], player['entryId'], player['position']), label
        assert sample['bulkReads'] == 0, label
        assert sample['syntheticCollection']['writes'] == (1 if label == 'saved' else 0), label
        assert sample['syntheticCollection']['creates'] == 0, label
    assert baseline['selected'] == rows['keyboard_ready']['selected'] == [1, 3]
    assert rows['pointer_picker']['selected'] == rows['pointer_cancelled']['selected'] == [2]
    assert rows['keyboard_picker']['selected'] == rows['saved']['selected'] == [1, 3]
    for label, indices in [('pointer_picker', [1]), ('keyboard_picker', [2, 0])]:
        receipt = rows[label]['picked']
        assert rows[label]['focusInPicker'] and receipt['total'] == len(indices)
        assert receipt['revision'] == queue['revision'] and receipt['epoch'] == queue['epoch']
        assert receipt['entryIds'] == [entries[index]['entryId'] for index in indices]
        assert receipt['ids'] == [entries[index]['track']['id'] for index in indices]
    assert complete['order'] == [rows['pointer_picker']['picked'], rows['keyboard_picker']['picked']]
    assert rows['pointer_cancelled']['focus'] == '收藏 ' + entries[1]['track']['name']
    assert rows['saved']['focus'] == '收藏 ' + entries[3]['track']['name']
    voice = rows['voice_excluded']['nativeQueue']['tracks']
    assert len(voice) == 2 and all(entry['track'].get('voiceId') and 'collectible' not in entry for entry in voice)
    server = complete['syntheticCollection']
    assert complete['audioFactoryCount'] >= 1
    assert server['writes'] == 1 and server['creates'] == 0
    [payload] = server['inlineAddPayloads']
    assert set(payload) == {'pid', 'trackIds', 'op', 'imme'}
    assert str(payload['pid']) == '2' and payload['op'] == 'add' and payload['imme'] is True
    assert list(map(str, json.loads(payload['trackIds']))) == rows['saved']['readback'] == ['3', '1']

    report = dict(ok=True, states=len(LABELS), actualWebView=True, trustedInput=args.trusted_input,
                  screenshots=len(LABELS) if args.screenshots else 0, pausedNativePlayerPreserved=True,
                  voiceExcluded=True, syntheticPlaylistWrites=1, realAccountWrites=0,
                  realMusicRequests=0, physicalAudioOutputVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--trusted-input', action='store_true',
                        help='actual XTest mouse/Return under a dedicated Xvfb (requires --screenshots)')
    options = parser.parse_args()
    if options.trusted_input and (not options.screenshots or os.name == 'nt'):
        parser.error('--trusted-input requires Linux screenshot capture and an isolated Xvfb')
    run(options)
