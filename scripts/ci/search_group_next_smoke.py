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
    env.update(
        CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
        CUMULUS_SEARCH_UI_PROOF='1', CUMULUS_SEARCH_GROUP_NEXT_PROOF='1',
        CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]
                              / 'desktop/examples/search_group_next_ui_probe.js'),
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
    assert report['ok'] and report['stageCount'] == len(LABELS)
    assert report['realMusicRequests'] == report['realAccountWrites'] == 0
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
    args = parser.parse_args()
    require_xvfb()
    run(args)


if __name__ == '__main__':
    main()
