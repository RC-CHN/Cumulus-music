"""Search song-tab selection and route-bound queue drop in a real WebView."""

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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/search_queue_drop_ui_probe.js'
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
            driver = TrustedDrag()
            stack.callback(driver.close)
            hook = driver.at_capture
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots,
                      before_ack=hook)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'search queue drop exited {code}'
    result = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert result['ok'] and result['actualNativePlayback'] and result['stageCount'] == len(LABELS)
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
    parser.add_argument('--trusted-pointer', action='store_true',
                        help='drag via XTest under a dedicated Xvfb (requires --screenshots)')
    args = parser.parse_args()
    if args.trusted_pointer and (not args.screenshots or not os.environ.get('DISPLAY')):
        parser.error('--trusted-pointer requires --screenshots and an isolated Xvfb DISPLAY')
    run(args)


if __name__ == '__main__':
    main()
