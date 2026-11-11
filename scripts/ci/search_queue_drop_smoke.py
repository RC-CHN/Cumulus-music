"""Song and Lyrics search selection and route-bound drop in a real WebView."""

import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path

from motion_smoke import launch


LABELS = (
    'paused_baseline',
    'reverse_selection',
    'ordered_drag_target',
    'insertion_boundary',
    'ordered_drop',
    'escape_cancelled',
    'origin_left',
)


LYRIC_LABELS = (
    'paused_baseline', 'group_selection', 'blocked_drag_target', 'blocked_marker',
    'blocked_committed', 'duplicate_drag_target', 'duplicate_marker',
    'duplicate_committed', 'stale_drag_target', 'stale_marker', 'origin_left', 'stale_released',
)


class TrustedDrag:
    """XTest on an isolated Xvfb only; driven by the screenshot handshake."""

    def __init__(self):
        import ctypes

        self.x11 = ctypes.CDLL('libX11.so.6')
        self.xtest = ctypes.CDLL('libXtst.so.6')
        self.x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
        self.x11.XOpenDisplay.restype = ctypes.c_void_p
        self.x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
        self.x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
        self.xtest.XTestFakeMotionEvent.argtypes = [
            ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong,
        ]
        self.xtest.XTestFakeButtonEvent.argtypes = [
            ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong,
        ]
        self.display = self.x11.XOpenDisplay(None)
        assert self.display, 'open the isolated X11 display'
        self.down = False

    def move(self, point):
        assert self.xtest.XTestFakeMotionEvent(self.display, -1, point['x'], point['y'], 0)
        self.x11.XSync(self.display, 0)

    def button(self, pressed):
        assert self.xtest.XTestFakeButtonEvent(self.display, 1, int(pressed), 0)
        self.x11.XSync(self.display, 0)
        self.down = pressed

    def at_capture(self, path, payload, _pid):
        if path.stem == 'ux_03_ordered_drag_target':
            self.move(payload['pointer']['from'])
            self.button(True)
            self.move(payload['pointer']['to'])
        elif path.stem == 'ux_04_insertion_boundary':
            assert self.down, 'trusted down precedes marker capture'
            self.button(False)

    def close(self):
        try:
            if self.down:
                self.button(False)
        finally:
            self.x11.XCloseDisplay(self.display)


class TrustedLyricDrag(TrustedDrag):
    def at_capture(self, _path, payload, _pid):
        label = payload['label']
        if label.endswith('_drag_target'):
            assert not self.down, 'previous gesture already released'
            self.move(payload['pointer']['from'])
            self.button(True)
            self.move(payload['pointer']['to'])
        elif label in ('blocked_marker', 'duplicate_marker', 'origin_left'):
            assert self.down, 'trusted pointer is still held'
            self.button(False)


def verify_lyrics(root, output, result, args):
    read = lambda path: json.loads(path.read_text(encoding='utf-8'))
    names = [f'ux_{index:02}_{label}' for index, label in enumerate(LYRIC_LABELS, 1)]
    assert sorted(path.stem for path in output.glob('ux_*.json')) == names
    assert sorted(path.stem for path in output.glob('ux_*.png')) == (names if args.screenshots else [])
    rows = [read(output / (name + '.json')) for name in names]
    assert [row['label'] for row in rows] == list(LYRIC_LABELS)
    assert result['stageCount'] == len(rows)
    assert result['trustedPointer'] is args.trusted_pointer
    if os.name != 'nt':
        assert read(output / 'locale.json') == dict(numeric='C', messages='en_US.UTF-8',
                                                    characters='en_US.UTF-8')

    before = rows[0]['nativePlayer']
    assert abs(before['position'] - 10) < .2 and before['mode'] == 'single_loop'
    original = rows[0]['nativeQueue']
    before_ids = ['92', '91', '93']
    front_ids = ['92', '91', '103', '101', '93']
    tail_ids = ['92', '91', '93', '103', '101']
    front = rows[4]['nativeQueue']
    tail = rows[7]['nativeQueue']
    assert len({original['revision'], front['revision'], tail['revision']}) == 3

    group = [dict(id='103', ordinal=2, right='playable'),
             dict(id='102', ordinal=1, right='copyright_blocked'),
             dict(id='101', ordinal=0, right='playable')]
    actions = [dict(anchor=group[1], rows=group, beforeEntry=original['tracks'][0]['entryId'],
                    queueRevision=original['revision'], playId=before['playId'], quality='standard'),
               dict(anchor=dict(group[-1], ordinal=3), rows=group, beforeEntry=None,
                    queueRevision=front['revision'], playId=before['playId'], quality='standard')]
    assert result['dropCalls'] == actions
    for index, row in enumerate([*rows, result]):
        expected = (before_ids, original, 0) if index < 4 else (
            (front_ids, front, 1) if index < 7 else (tail_ids, tail, 2))
        ids, snapshot, number = expected
        player, queue = row['nativePlayer'], row['nativeQueue']
        assert [entry['track']['id'] for entry in queue['tracks']] == ids
        assert queue == snapshot and row['dropCalls'] == actions[:number]
        assert queue['revision'] == player['queueRevision'] and queue['current'] == before['entryId']
        assert player['state'] == 'paused' and player['track']['id'] == '91'
        assert all(player[key] == before[key] for key in ('epoch', 'playId', 'entryId', 'position'))
        assert row['audioFactoryCount'] == 1
        if index < len(rows):
            assert row['checkedPlayer']['queueRevision'] == player['queueRevision']
            assert row['checkedQueue'] == queue
        fixture = row['searchLyricGroupFixture']
        assert fixture == dict(lyrics=[dict(path='/api/search/resource/lyric',
            args=dict(keyword='合成歌词拖入队列', scene='normal', limit=10, offset=0), cached=False)],
            media=[dict(id=91, trialMode=-1, level='standard')], rowCounts=[])
    assert rows[0]['selectedOrdinals'] == rows[-1]['selectedOrdinals'] == []
    assert all(row['selectedOrdinals'] == [0, 1, 2, 3] for row in rows[1:10])
    assert [rows[index]['markerTop'] for index in (3, 6, 9)] == ['0px', '320px', '0px']
    for index in (3, 6, 9):
        assert rows[index]['nativeDown'] is args.trusted_pointer
        if args.screenshots:
            samples = read(output / f'paint-{names[index]}.json')['samples']
            assert len(samples) == 1 and samples[0]['ok']
    assert all(rows[index]['nativeUp'] is args.trusted_pointer for index in (4, 7, 11))
    assert [(row['track']['id'], row['entryId']) for row in original['tracks']] == [
        (row['track']['id'], row['entryId']) for row in tail['tracks'][:3]]
    assert [row['entryId'] for row in front['tracks'][2:4]] == [row['entryId'] for row in tail['tracks'][3:]]
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0
    audit = dict(ok=True, actualWebView=True, actualNativePlayback=True, orderedQueue=tail_ids,
                 frontVisualGapAfterCurrent=True, blockedGrabbedRowFiltered=True,
                 exactDuplicateHit=actions[1]['anchor'], orderedSelections=group,
                 noAdditionalMediaAuthorization=True, staleReleaseIgnored=True,
                 pausedPosition=before['position'], trustedPointer=args.trusted_pointer,
                 physicalDisplayVerified=False, physicalAudioOutputVerified=False,
                 screenshots=len(rows) if args.screenshots else 0, realMusicRequests=0, realAccountWrites=0)
    (root / 'audit.json').write_text(json.dumps(audit, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(audit, ensure_ascii=False))


def run(args):
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_SEARCH_UI_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.lyrics:
        env['CUMULUS_SEARCH_LYRIC_GROUP_PROOF'] = '1'
        env['CUMULUS_SEARCH_LYRIC_DROP_PROOF'] = '1'
    script_name = 'search_lyric_queue_drop_ui_probe.js' if args.lyrics else 'search_queue_drop_ui_probe.js'
    source = Path(__file__).resolve().parents[2] / 'desktop/examples' / script_name
    script = root / 'search-queue-drop.js'
    script.write_text(
        'window.__cumulusSearchDropTrusted = ' + json.dumps(args.trusted_pointer) + ';\n'
        + source.read_text(encoding='utf-8'),
        encoding='utf-8',
    )
    env['CUMULUS_UX_SCRIPT'] = str(script)
    output = root / 'first'
    with ExitStack() as stack:
        hook = None
        if args.trusted_pointer:
            driver = TrustedLyricDrag() if args.lyrics else TrustedDrag()
            stack.callback(driver.close)
            hook = driver.at_capture
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots,
                      before_ack=hook, acknowledge_receipts=args.lyrics)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'search queue drop exited {code}'
    result = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert result['ok'] and result['actualNativePlayback']
    if args.lyrics:
        verify_lyrics(root, output, result, args)
        return
    assert result['stageCount'] == len(LABELS)
    assert result['trustedPointer'] == args.trusted_pointer
    assert result['dropCommands'] == 1
    assert result['realMusicRequests'] == result['realAccountWrites'] == 0

    before = after = None
    for index, label in enumerate(LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        record = json.loads(path.read_text(encoding='utf-8'))
        player, queue = record['checkedPlayer'], record['checkedQueue']
        ids = [item['track']['id'] for item in queue['tracks']]
        if index == 1:
            before = player
        if index == 5:
            after = queue['revision']
            assert after != before['queueRevision']
        assert player['state'] == 'paused' and abs(player['position'] - 10) < .2
        for key in ('playId', 'entryId', 'position'):
            assert player[key] == before[key], (label, key)
        assert queue['current'] == before['entryId']
        assert queue['revision'] == player['queueRevision']
        assert ids == (['701', '702'] if index < 5 else ['701', '102', '101', '702'])
        assert queue['revision'] == (before['queueRevision'] if index < 5 else after)
        if 2 <= index <= 4:
            assert record['selected'] == ['101', '102']
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    if args.screenshots:
        pixels = json.loads((output / 'paint-ux_04_insertion_boundary.json').read_text(encoding='utf-8'))
        assert len(pixels['samples']) == 1 and pixels['samples'][0]['ok']
    result.update(actualWebView=True, orderedQueue=['701', '102', '101', '702'],
                  pausedPosition=before['position'], physicalDisplayVerified=False,
                  physicalAudioOutputVerified=False, windowsVisualVerified=False,
                  screenshots=len(LABELS) if args.screenshots else 0)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--lyrics', action='store_true', help='exercise dedicated lyric rows and their distinct grants')
    parser.add_argument('--trusted-pointer', action='store_true',
                        help='drag via XTest under a dedicated Xvfb (requires --screenshots)')
    args = parser.parse_args()
    if args.trusted_pointer and (not args.screenshots or not os.environ.get('DISPLAY')):
        parser.error('--trusted-pointer requires --screenshots and an isolated Xvfb DISPLAY')
    if args.lyrics and args.trusted_pointer:
        from vinyl_lyric_copy_smoke import require_xvfb
        require_xvfb()
    run(args)


if __name__ == '__main__':
    main()
