"""Linux Xvfb: real copy gestures across vinyl display modes, using GTK clipboard."""

import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import time

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import TrustedLyricInput, require_xvfb
from window_manager import launch


SONG = '合成歌曲 2 · A quiet afternoon'
CASES = {'classic_title': ('classic', SONG), 'cover_preview': ('cover', '合成歌词 4'),
         'lyric_title': ('lyric', SONG)}
STEPS = ('drag_ready', 'copy_ready', 'copied')
NEGATIVE_STAGES = ('classic_source_drag_ready', 'classic_source_excluded')


class TrustedVinylTextInput(TrustedLyricInput):
    def __init__(self):
        super().__init__()
        self.readbacks = {}

    def drag(self, path, payload, pid, case, sentinel):
        self.clipboard.set_text(sentinel, -1)
        assert self.read_clipboard() == sentinel
        start, end = (self.pointer_target(payload['targets'][key], pid) for key in ('from', 'to'))
        assert start['clientX'] == end['clientX'] and end['x'] - start['x'] > 15, (start, end)
        path.with_name(f'{case}-trusted-pointer.json').write_text(
            json.dumps({'from': start, 'to': end}, indent=2) + '\n', encoding='utf-8')
        self.move(start)
        self.button(True)
        try:
            time.sleep(.06)
            for step in range(1, 13):
                self.move({axis: round(start[axis] + (end[axis] - start[axis]) * step / 12)
                           for axis in ('x', 'y')})
                time.sleep(.02)
        finally:
            self.button(False)

    def at_capture(self, path, payload, pid):
        label = path.stem.split('_', 2)[2]
        if label in NEGATIVE_STAGES:
            assert payload['mode'] == 'classic'
            sentinel = 'cumulus sealed excluded metadata'
            if label == NEGATIVE_STAGES[0]:
                self.drag(path, payload, pid, 'classic_source', sentinel)
            else:
                assert payload['selection']['collapsed'] and self.read_clipboard() == sentinel
            return
        case, step = next((case, step) for case in CASES for step in STEPS
                          if label == f'{case}_{step}')
        mode, expected = CASES[case]
        assert payload['mode'] == mode
        sentinel = f'cumulus sealed vinyl copy: {case}'
        if step == 'drag_ready':
            self.drag(path, payload, pid, case, sentinel)
        elif step == 'copy_ready':
            assert self.read_clipboard() == sentinel, 'drag alone changed the clipboard'
            self.key('Control_L', 'c')
        else:
            receipt = {'clipboard': self.read_clipboard(), 'richHtmlPresent': self.clipboard_has_html()}
            path.with_name(f'{case}-clipboard.json').write_text(
                json.dumps(receipt, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
            assert receipt == {'clipboard': expected + '\n', 'richHtmlPresent': False}, (case, receipt)
            self.readbacks[case] = receipt


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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/vinyl_text_copy_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_PLAYER_LAYOUT_PROOF='1',
               CUMULUS_QUEUE_UI_PROOF='1', CUMULUS_UX_SCRIPT=str(source),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    isolated_profile(env, root)
    output = root / 'first'
    with ExitStack() as stack:
        driver = TrustedVinylTextInput()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        status = launch(args.binary.resolve(strict=True), env, log, output, True, before_ack=driver.at_capture)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exited {status}'
    complete = read(output / 'complete.json')
    cases = [(label, 'classic') for label in NEGATIVE_STAGES]
    cases += [(f'{case}_{step}', CASES[case][0]) for case in CASES for step in STEPS]
    expected = len(cases)
    assert complete['ok'] and complete['stageCount'] == expected and complete['excludedClassicMetadata']
    assert complete['realMusicRequests'] == complete['accountWrites'] == 0
    assert len(list(output.glob('ux_*.json'))) == len(list(output.glob('ux_*.png'))) == expected
    baseline = None
    for index, (label, mode) in enumerate(cases, 1):
        stage = read(output / f'ux_{index:02d}_{label}.json')
        native = stage['nativePlayer']
        baseline = baseline or stage
        assert native['state'] == 'paused' and not native['nativeSeeking'] and not native['error']
        assert native['track']['id'] == '2' and native['playId'] and native['entryId']
        assert stage['nativeQueue'] == baseline['nativeQueue']
        assert all(native[key] == baseline['nativePlayer'][key] for key in
                   ('playId', 'entryId', 'queueRevision', 'position'))
        assert abs(native['position'] - 4.25) < .12 and stage['mode'] == mode
        if label.endswith('_copied'):
            assert stage['copy'] == {'trusted': True, 'handled': True, 'hasClipboardData': True}
    assert list(driver.readbacks) == list(CASES)
    report = {'ok': True, 'actualWebView': True, 'trustedX11': True, 'cases': driver.readbacks,
              'nativePosition': baseline['nativePlayer']['position'], 'nativePlayIdUnchanged': True,
              'nativeQueueUnchanged': True, 'excludedClassicMetadata': True, 'screenshots': expected,
              'realMusicRequests': 0, 'accountWrites': 0, 'physicalAudioOutputVerified': False}
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
