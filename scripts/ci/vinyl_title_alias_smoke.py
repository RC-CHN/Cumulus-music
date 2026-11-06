"""Linux Xvfb: standard vinyl aliases, title clamping and trusted plain copy."""

import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from vinyl_text_copy_smoke import TrustedVinylTextInput, read
from window_manager import launch


TRANSLATION = '合成翻译；另一译名'
FALLBACK = '备用曲名；录音室版'
SENTINEL = 'cumulus sealed alias clipboard: untouched'
STAGES = (
    ('precedence_drag_ready', '2', 'classic', TRANSLATION),
    ('precedence_copy_ready', '2', 'classic', TRANSLATION),
    ('precedence_copied', '2', 'classic', TRANSLATION),
    ('precedence_cover', '2', 'cover', None),
    ('precedence_lyric', '2', 'lyric', None),
    ('precedence_classic_return', '2', 'classic', TRANSLATION),
    ('fallback_classic', '3', 'classic', FALLBACK),
    ('empty_tns_classic', '4', 'classic', None),
)
ALIASES = {'2': TRANSLATION, '3': FALLBACK, '4': None}


class TrustedAliasInput(TrustedVinylTextInput):
    def at_capture(self, path, payload, pid):
        label = path.stem.split('_', 2)[2]
        if label == 'precedence_drag_ready':
            self.drag(path, payload, pid, 'precedence_alias', SENTINEL)
        elif label == 'precedence_copy_ready':
            assert self.read_clipboard() == SENTINEL, 'drag alone changed the clipboard'
            assert payload['selection']['onAlias']
            assert payload['selection']['text'].strip() == TRANSLATION
            self.key('Control_L', 'c')
        elif label == 'precedence_copied':
            receipt = {'clipboard': self.read_clipboard(), 'richHtmlPresent': self.clipboard_has_html()}
            path.with_name('precedence-alias-clipboard.json').write_text(
                json.dumps(receipt, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
            assert receipt == {'clipboard': TRANSLATION + '\n', 'richHtmlPresent': False}, receipt
            self.readbacks['alias'] = receipt


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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/vinyl_title_alias_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_PLAYER_LAYOUT_PROOF='1',
               CUMULUS_QUEUE_UI_PROOF='1', CUMULUS_VINYL_TITLE_ALIAS_PROOF='1',
               CUMULUS_UX_SCRIPT=str(source),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    isolated_profile(env, root)
    output = root / 'first'
    with ExitStack() as stack:
        driver = TrustedAliasInput()
        stack.callback(driver.close)
        log = stack.enter_context((root / 'host.log').open('x', encoding='utf-8'))
        status = launch(args.binary.resolve(strict=True), env, log, output, True, before_ack=driver.at_capture)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exited {status}'
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['stageCount'] == len(STAGES)
    assert complete['realMusicRequests'] == complete['accountWrites'] == 0
    assert len(list(output.glob('ux_*.json'))) == len(list(output.glob('ux_*.png'))) == len(STAGES)
    assert set(driver.readbacks) == {'alias'}
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] not in ('C', 'POSIX') for key in ('messages', 'characters'))

    baselines = {}
    for index, (label, track_id, mode, visible_alias) in enumerate(STAGES, 1):
        stage = read(output / f'ux_{index:02d}_{label}.json')
        native = stage['nativePlayer']
        assert native['track']['id'] == track_id and native.get('titleAlias') == ALIASES[track_id]
        assert native['state'] == 'paused' and not native['nativeSeeking'] and not native['error']
        assert native['playId'] and native['entryId'] and abs(native['position'] - 4.25) < .12
        assert stage['mode'] == mode
        assert stage['layout'] == {
            'title': native['track']['name'], 'alias': visible_alias,
            'marked': visible_alias is not None, 'clamp': '2' if visible_alias is None else '1',
        }
        signature = tuple(native[key] for key in ('playId', 'entryId', 'queueRevision', 'position'))
        signature += (stage['nativeQueue'],)
        assert baselines.setdefault(track_id, signature) == signature
        if label == 'precedence_copied':
            assert stage['copy'] == {'trusted': True, 'handled': True, 'hasClipboardData': True}
    assert len({signature[0] for signature in baselines.values()}) == 3
    report = {'ok': True, 'actualWebView': True, 'actualPausedPlayback': True, 'trustedX11': True,
              'clipboard': driver.readbacks['alias'], 'nativeAliases': ALIASES,
              'classicSingleLineWithAliasOnly': True, 'alternateModesHideAlias': True,
              'nativePlayQueueAndClockUnchangedAcrossModes': True, 'screenshots': len(STAGES),
              'realMusicRequests': 0, 'accountWrites': 0, 'physicalAudioOutputVerified': False}
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
