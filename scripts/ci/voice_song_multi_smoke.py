"""Actual episode song selection via XTest in a dedicated Linux WebView."""

import argparse
from contextlib import ExitStack
import ctypes
import json
import os
from pathlib import Path
import time

from queue_collection_smoke import isolated_profile
from queue_row_collection_smoke import QueueCollectInput
from vinyl_lyric_copy_smoke import require_xvfb
from voice_songs_smoke import MEDIA, TRACKS, pair_ids, read
from window_manager import launch

CASES = {
    'menu': ('baseline', 'first_row', 'control_row', 'control_denied', 'menu_next_open',
             'menu_next_choose', 'next_queued', 'menu_play_open', 'menu_play_choose', 'playing'),
    'replace': ('baseline', 'first_row', 'shift_double', 'playing'),
    'add': ('baseline', 'first_row', 'shift_double', 'playing'),
    'cover': ('baseline', 'first_row', 'control_row', 'cover_choose', 'playing'),
}
ORIGINAL = [('1601', '601'), ('1602', '602')]
QUEUED = [('1601', '601'), ('1601', None), ('1710', None), ('1602', '602')]


class TrustedSongInput(QueueCollectInput):
    def __init__(self):
        super().__init__()
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.restype = ctypes.c_uint8
        self.xtest.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int,
                                                ctypes.c_ulong]
        self.xtest.XTestFakeKeyEvent.restype = ctypes.c_int

    def key(self, code, pressed):
        assert self.xtest.XTestFakeKeyEvent(self.display, code, int(pressed), 0)
        self.x11.XSync(self.display, 0)

    def at_capture(self, path, payload, pid):
        request = payload.get('input')
        if request is None:
            return
        point = self.pointer_target(payload['pointer'], pid)
        assert point['clientX'] <= point['x'] < point['clientX'] + point['clientWidth']
        assert point['clientY'] <= point['y'] < point['clientY'] + point['clientHeight']
        assert set(request) == {'target', 'button', 'ctrl', 'shift', 'count'}
        assert request['button'] in (1, 3) and request['count'] in (1, 2)
        assert isinstance(request['ctrl'], bool) and isinstance(request['shift'], bool)
        assert request['count'] == 1 or request['button'] == 1
        self.move(point)
        held = []
        try:
            for keysym, enabled in ((0xffe3, request['ctrl']), (0xffe1, request['shift'])):
                if enabled:
                    code = self.x11.XKeysymToKeycode(self.display, keysym)
                    assert code
                    self.key(code, True)
                    held.append(code)
            for index in range(request['count']):
                button = request['button']
                assert self.xtest.XTestFakeButtonEvent(self.display, button, 1, 0)
                self.x11.XSync(self.display, 0)
                try:
                    time.sleep(.04)
                finally:
                    assert self.xtest.XTestFakeButtonEvent(self.display, button, 0, 0)
                    self.x11.XSync(self.display, 0)
                if index + 1 < request['count']:
                    time.sleep(.06)
        finally:
            for code in reversed(held):
                self.key(code, False)
        path.with_name('input-' + path.name).write_text(
            json.dumps({'request': request, 'nativePoint': point}, indent=2) + '\n',
            encoding='utf-8',
        )


def verify(case, output):
    paths = sorted(output.glob('ux_*.json'))
    rows = {path.stem.split('_', 2)[2]: read(path) for path in paths}
    assert tuple(rows) == CASES[case], list(rows)
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['mode'] == case and complete['count'] == len(paths)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    assert all(stage['actual'] and stage['mode'] == case and not stage['player']['error']
               for stage in rows.values())
    before, final = rows['baseline'], rows['playing']
    original, current = before['player'], final['player']
    assert before['replace'] == (case != 'add')
    assert original['kind'] == 'voice' and original['state'] == 'paused' and original['voiceId'] == '601'
    assert original['position'] > .2 and not original['seeking'] and pair_ids(before['queue']) == ORIGINAL
    assert current['kind'] == 'catalog' and current['state'] == 'playing' and current['voiceId'] is None
    assert current['position'] > .2 and not current['seeking']
    assert current['playId'] != original['playId'] and current['entryId'] != original['entryId']
    assert final['queue']['current'] == current['entryId']
    assert len(before['reads']) == len(final['reads']) == 1
    episode_read = before['reads'][0]
    epoch, play_id = episode_read['args']
    page = episode_read['result']
    receipt = page['receipt']
    assert play_id == original['playId'] == page['playId'] and page['voiceId'] == '601' and receipt

    for path, row in zip(paths, rows.values(), strict=True):
        assert path.with_suffix('.png').is_file()
        if 'input' not in row:
            continue
        native_input = read(path.with_name('input-' + path.name))
        assert native_input['request'] == row['input']
        spec = row['input']
        button = 2 if spec['button'] == 3 else 0
        kind = 'contextmenu' if button == 2 else 'dblclick' if spec['count'] == 2 else 'click'
        assert any(event['trusted'] and event['type'] == kind and event['target'] == spec['target'] and
                   event['button'] == button and event['ctrl'] == spec['ctrl'] and event['shift'] == spec['shift']
                   for event in final['events'][len(row['events']):]), (spec, final['events'])

    def action(kind, index, indices):
        return [epoch, play_id, receipt, {'kind': kind, 'index': index, 'indices': indices}, 'standard']

    selected_calls = [call['args'] for call in final['selectedActions']]
    singles = [call['args'] for call in final['singleActions']]
    assert all(call['result'] and 'error' not in call for call in
               (*final['selectedActions'], *final['singleActions']))
    if case == 'menu':
        assert not singles and selected_calls == [
            action('next_play', 0, [1, 0, 2]), action('menu_play', 0, [1, 0, 2]),
        ]
        assert rows['menu_next_open']['selected'] == rows['menu_next_choose']['selected'] == [0, 1, 2]
        queued = rows['next_queued']
        assert pair_ids(queued['queue']) == pair_ids(final['queue']) == QUEUED
        assert queued['queue']['current'] == before['queue']['current'] and \
            queued['queue']['entries'][::3] == before['queue']['entries']
        assert queued['queue']['entries'] == final['queue']['entries']
        assert queued['queue']['revision'] != before['queue']['revision']
        assert {k: v for k, v in queued['player'].items() if k != 'queueRevision'} == {
            k: v for k, v in original.items() if k != 'queueRevision'
        }
        assert current['trackId'] == '1710'
    elif case == 'cover':
        assert not selected_calls and singles == [
            [epoch, play_id, receipt, {'kind': 'cover', 'index': 0}, 'standard'],
        ]
        assert rows['cover_choose']['selected'] == [0, 1]
        assert pair_ids(final['queue']) == [('1601', '601'), ('1710', None), ('1602', '602')]
        assert current['trackId'] == '1710'
    else:
        assert not singles and selected_calls == [action('double_click', 1, [0, 1])]
        assert pair_ids(final['queue']) == ([('1710', None), ('1601', None)] if case == 'replace' else
                                           [('1601', '601'), ('1601', None), ('1602', '602')])
        assert current['trackId'] == '1601'
    native = complete['voiceSongsTrace']
    assert native['songReads'] == [[TRACKS, {'id': '601'}]] and not native['likes'] and not native['liked']
    assert all(path == MEDIA and body['level'] == 'standard' and
               [str(value) for value in json.loads(body['ids'])] == [song_id] and
               song_id in ('1601', '1602', '1710') for path, body, song_id in native['mediaReads'])
    assert current['trackId'] in [song_id for _, _, song_id in native['mediaReads']]
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    return {'case': case, 'stages': len(paths), 'screenshots': len(paths),
            'selectedActions': len(selected_calls), 'singleActions': len(singles)}


def run(args, case, root):
    root.mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_SONGS_PROOF='double',
               CUMULUS_PODCAST_SCENE_HOME_PROOF='automatic', LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/voice_song_multi_ui_probe.js'
    script = root / source.name
    script.write_text('window.__voiceSongsMultiCase=' + json.dumps(case) + ';\n' +
                      source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    isolated_profile(env, root)
    output = root / 'run/first'
    with ExitStack() as stack:
        driver = TrustedSongInput()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        result = launch(args.binary.resolve(strict=True), env, log, output, True,
                        before_ack=driver.at_capture, capture_glob='ux_[0-9][0-9]_*.json')
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, f'desktop native proof exited {result}'
    return verify(case, output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--case', choices=('all', *CASES), default='all')
    args = parser.parse_args()
    require_xvfb()
    if os.environ.get('CUMULUS_TEST_WINDOW_MANAGER') != 'openbox':
        parser.error('set CUMULUS_TEST_WINDOW_MANAGER=openbox on the dedicated Xvfb display')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    selected = CASES if args.case == 'all' else (args.case,)
    report = {'actualWebView': 'WebKitGTK', 'trustedX11Pointer': True, 'actualNullAudio': True,
              'cases': [run(args, case, root / case) for case in selected], 'realMusicRequests': 0,
              'realAccountWrites': 0, 'physicalAudioVerified': False}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
