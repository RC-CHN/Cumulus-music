"""Native, receipt-scoped search group Next in a real Linux GTK WebView."""

import argparse
import ctypes
from contextlib import ExitStack
import json
import os
from pathlib import Path

from search_queue_drop_smoke import TrustedDrag
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch


LABELS = (
    'select_first', 'select_blocked', 'select_third',
    'open_blocked_anchor', 'blocked_anchor_menu',
    'open_playable_anchor', 'group_menu', 'group_inserted', 'single_menu', 'single_inserted',
)

EXTENDED_LABELS = (
    'select_first', 'select_blocked', 'select_third', 'open_empty_anchor', 'empty_menu',
    'empty_started', 'select_random_first', 'select_random_anchor', 'open_random_anchor',
    'random_menu', 'random_inserted',
)


def verify_extended(output, report):
    previous = None
    search_calls = None
    for index, label in enumerate(EXTENDED_LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        receipt = json.loads(path.read_text(encoding='utf-8'))
        assert path.with_suffix('.png').is_file()
        if label in ('empty_menu', 'random_menu'):
            pixels = json.loads((output / f'paint-{path.name}').read_text(encoding='utf-8'))
            assert len(pixels['samples']) == 1 and pixels['samples'][0]['ok']
        player, queue = receipt['checkedPlayer'], receipt['checkedQueue']
        assert player['mode'] == 'random' and not player.get('error')
        assert player['queueRevision'] == queue['revision']
        if search_calls is None:
            search_calls = receipt['syntheticSearchCalls']
        assert receipt['syntheticSearchCalls'] == search_calls
        ids = [item['track']['id'] for item in queue['tracks']]
        media = receipt['searchGroupNextMedia']
        if index <= 5:
            assert player['state'] == 'idle' and player['entryId'] is player['playId'] is None
            assert queue['current'] is None and ids == [] and media == []
        else:
            assert player['track']['id'] == '101' and queue['current'] == player['entryId']
            assert media[0] == ['101', 42] and media.count(['101', 42]) == 1
            assert all(song in ('101', '103', '104', '105') and trial == 42 for song, trial in media)
            if index == 6:
                assert player['state'] == 'playing' and player['position'] > .2
                previous = player
            else:
                assert player['state'] == 'paused' and player['position'] >= previous['position']
                assert all(player[field] == previous[field] for field in ('playId', 'entryId'))
            assert ids == (['103', '101', '104', '105'] if index == 11 else ['103', '101'])
        if index == 11:
            assert player['queueRevision'] != previous['queueRevision']
            commands = receipt['actionTrace']
            assert len(commands) == 2 and all(c['outcome'] == 'confirmed' for c in commands)
            for command, anchor, chosen in zip(commands, ['101', '105'],
                                                [['103', '102', '101'], ['104', '105']]):
                action = command['action']
                assert action['kind'] == 'search_group_next_play'
                assert action['anchor']['track_id'] == anchor
                assert [r['track_id'] for r in action['rows']] == chosen
            assert commands[0]['action']['expected_play_id'] is None
            assert commands[1]['action']['expected_play_id'] == previous['playId']
    report.update(finalQueue=['103', '101', '104', '105'], startedAnchor='101',
                  screenshots=len(EXTENDED_LABELS))


PLAY_LABELS = (
    'select_first', 'select_blocked', 'select_third', 'open_blocked_play_anchor',
    'blocked_play_menu', 'open_new_play_anchor', 'group_play_menu', 'group_play_started',
    'select_resume_first', 'select_resume_current', 'open_current_play_anchor',
    'current_play_menu', 'current_play_resumed',
)


def verify_play(output, report):
    base = started = paused = None
    for index, label in enumerate(PLAY_LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        receipt = json.loads(path.read_text(encoding='utf-8'))
        assert path.with_suffix('.png').is_file()
        if label in ('blocked_play_menu', 'group_play_menu', 'current_play_menu'):
            pixels = json.loads((output / f'paint-{path.name}').read_text(encoding='utf-8'))
            assert len(pixels['samples']) == 1 and pixels['samples'][0]['ok']
        player, queue = receipt['checkedPlayer'], receipt['checkedQueue']
        assert player['mode'] == 'sequential' and not player.get('error')
        assert player['queueRevision'] == queue['revision'] and queue['current'] == player['entryId']
        ids = [item['track']['id'] for item in queue['tracks']]
        media = receipt['searchGroupNextMedia']
        if index == 1:
            base = player
            search_calls = receipt['syntheticSearchCalls']
        assert receipt['syntheticSearchCalls'] == search_calls
        if index <= 7:
            assert player['state'] == 'paused'
            assert all(player[field] == base[field] for field in
                       ('playId', 'entryId', 'position', 'queueRevision'))
            assert ids == ['701'] and media == []
            continue
        assert media == [['101', 42]]
        assert player['track']['id'] == '101'
        if label == 'group_play_started':
            assert player['state'] == 'playing' and player['position'] > .3
            assert all(player[field] != base[field] for field in
                       ('playId', 'entryId', 'queueRevision'))
            started = player
        assert all(player[field] == started[field] for field in ('playId', 'entryId'))
        if index < len(PLAY_LABELS):
            assert ids == ['701', '103', '101']
            if index == 9:
                paused = player
            if index >= 9:
                assert player['state'] == 'paused' and player['position'] == paused['position']
                assert player['position'] >= started['position']
        else:
            assert ids == ['701', '103', '104', '101']
            assert player['state'] == 'playing' and player['position'] > paused['position']
            assert player['queueRevision'] != started['queueRevision']
            commands = receipt['actionTrace']
            assert [c['outcome'] for c in commands] == [
                'search_group_anchor_unavailable', 'confirmed', 'confirmed',
            ]
            for command, anchor, chosen, observed in zip(
                commands, ['102', '101', '101'],
                [['103', '102', '101'], ['103', '102', '101'], ['104', '101']],
                [base, base, paused],
            ):
                action = command['action']
                assert action['kind'] == 'search_group_play'
                assert action['anchor']['track_id'] == anchor
                assert [r['track_id'] for r in action['rows']] == chosen
                assert action['expected_play_id'] == observed['playId']
                assert action['expected_queue_revision'] == observed['queueRevision']
    report.update(finalQueue=['701', '103', '104', '101'], startedAnchor='101',
                  resumedSameDecoder=True, screenshots=len(PLAY_LABELS))


class TrustedClicks(TrustedDrag):
    def __init__(self):
        super().__init__()
        self.x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        self.x11.XKeysymToKeycode.restype = ctypes.c_ubyte
        self.xtest.XTestFakeKeyEvent.argtypes = [
            ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong,
        ]
        self.control = self.x11.XKeysymToKeycode(self.display, 0xFFE3)  # XK_Control_L
        assert self.control

    def at_capture(self, _path, payload, _pid):
        pointer = payload.get('pointer')
        if pointer is None:
            return
        button = pointer['button']
        assert button in (1, 3)
        self.move(pointer)
        control = pointer['ctrl']
        if control:
            assert self.xtest.XTestFakeKeyEvent(self.display, self.control, 1, 0)
        try:
            assert self.xtest.XTestFakeButtonEvent(self.display, button, 1, 0)
            assert self.xtest.XTestFakeButtonEvent(self.display, button, 0, 0)
        finally:
            if control:
                assert self.xtest.XTestFakeKeyEvent(self.display, self.control, 0, 0)
            self.x11.XSync(self.display, 0)


def run(args):
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/search_group_next_ui_probe.js'
    if args.extended or args.play:
        combined = root / 'scoped-probe.js'
        variant = 'EXTENDED' if args.extended else 'PLAY'
        combined.write_text(f'window.__CUMULUS_SEARCH_GROUP_{variant}__ = true;\n' +
                            script.read_text(encoding='utf-8'), encoding='utf-8')
        script = combined
    env.update(
        CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
        CUMULUS_SEARCH_UI_PROOF='1', CUMULUS_SEARCH_GROUP_NEXT_PROOF='1',
        CUMULUS_UX_SCRIPT=str(script),
    )
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))

    output = root / 'first'
    with ExitStack() as stack:
        driver = TrustedClicks()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, output, True,
                      before_ack=driver.at_capture)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'search group Next exited {code}'
    report = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    labels = PLAY_LABELS if args.play else EXTENDED_LABELS if args.extended else LABELS
    assert report['ok'] and report['stageCount'] == len(labels)
    assert report['realMusicRequests'] == report['realAccountWrites'] == 0
    if args.extended or args.play:
        (verify_extended if args.extended else verify_play)(output, report)
        report.update(actualWebView=True, trustedPointer=True, physicalAudioOutputVerified=False,
                      windowsVisualVerified=False)
        (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
        return
    base = first_revision = None
    initial_search_calls = None
    for index, label in enumerate(LABELS, 1):
        path = output / f'ux_{index:02}_{label}.json'
        receipt = json.loads(path.read_text(encoding='utf-8'))
        assert path.with_suffix('.png').is_file()
        if label in ('blocked_anchor_menu', 'group_menu', 'single_menu'):
            pixels = json.loads((output / f'paint-{path.name}').read_text(encoding='utf-8'))
            assert len(pixels['samples']) == 1 and pixels['samples'][0]['ok']
        player, queue = receipt['checkedPlayer'], receipt['checkedQueue']
        if index == 1:
            base = player
            initial_search_calls = receipt['syntheticSearchCalls']
        assert receipt['syntheticSearchCalls'] == initial_search_calls
        assert player['state'] == 'paused' and player['mode'] == 'single_loop'
        assert all(player[field] == base[field] for field in ('position', 'playId', 'entryId'))
        assert player['queueRevision'] == queue['revision'] and queue['current'] == base['entryId']
        ids = [item['track']['id'] for item in queue['tracks']]
        if index < 8:
            assert ids == ['701', '702'] and queue['revision'] == base['queueRevision']
        elif index < 10:
            assert ids == ['701', '103', '101', '702'] and queue['revision'] != base['queueRevision']
            first_revision = queue['revision']
        else:
            assert ids == ['701', '104', '103', '101', '702'] and queue['revision'] != first_revision
            commands = receipt['actionTrace']
            assert [c['action']['kind'] for c in commands] == [
                'search_group_next_play', 'search_group_next_play', 'search_next_play',
            ]
            assert commands[0]['outcome'] == 'search_group_anchor_unavailable'
            assert all(c['outcome'] == 'confirmed' for c in commands[1:])
            assert [c['action']['anchor']['track_id'] for c in commands[:2]] == ['102', '101']
            assert [[r['track_id'] for r in c['action']['rows']] for c in commands[:2]] == [
                ['103', '102', '101'], ['103', '102', '101'],
            ]
            assert commands[2]['action']['id'] == '104'
    report.update(actualWebView=True, trustedPointer=True, screenshots=len(LABELS),
                  finalQueue=['701', '104', '103', '101', '702'], physicalAudioOutputVerified=False,
                  windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--extended', action='store_true', help='prove empty and random Next')
    mode.add_argument('--play', action='store_true', help='prove explicit group Play')
    args = parser.parse_args()
    require_xvfb()
    run(args)


if __name__ == '__main__':
    main()
