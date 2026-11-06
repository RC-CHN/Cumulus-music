"""Read-only embedded comment copy through a real Xvfb pointer and GTK clipboard."""

import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from vinyl_text_copy_smoke import TrustedVinylTextInput, read
from window_manager import launch


EXPECTED = '合成评论 101\n'
SENTINEL = 'cumulus sealed comment clipboard: untouched'
LABELS = ('comment_drag_ready', 'comment_copy_ready', 'comment_copied')
COUNT = '/api/resource/commentInfo/list'
LATEST = '/api/v1/resource/comments/R_SO_4_1'


class TrustedCommentInput(TrustedVinylTextInput):
    def at_capture(self, path, payload, pid):
        label = path.stem.split('_', 2)[2]
        assert payload['pane'] == 'comments' and payload['visible']
        if label == 'comment_drag_ready':
            self.drag(path, payload, pid, 'comment_body', SENTINEL)
        elif label == 'comment_copy_ready':
            assert payload['selection'] == {'text': EXPECTED.rstrip('\n'), 'collapsed': False, 'bodyOnly': True}
            assert self.read_clipboard() == SENTINEL, 'drag alone leaves the clipboard untouched'
            self.key('Control_L', 'c')
        elif label == 'comment_copied':
            self.copied = self.read_clipboard()
            self.rich_html_present = self.clipboard_has_html()
            receipt = {'clipboard': self.copied, 'richHtmlPresent': self.rich_html_present}
            path.with_name('comment_body-clipboard.json').write_text(
                json.dumps(receipt, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
            assert receipt == {'clipboard': EXPECTED, 'richHtmlPresent': False}, receipt
        else:
            raise AssertionError(f'unexpected comment stage {label}')


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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/vinyl_comment_copy_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_QUEUE_UI_PROOF='1',
               CUMULUS_VINYL_COMMENTS_PROOF='t1', CUMULUS_UX_SCRIPT=str(source),
               CUMULUS_PROOF_FRONTEND_DIST=str(args.frontend_dist.resolve(strict=True)))
    isolated_profile(env, root)
    output = root / 'first'
    with ExitStack() as stack:
        driver = TrustedCommentInput()
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
    assert abs(baseline['position'] - 4.25) < .12
    for row in stages:
        native = row['nativePlayer']
        assert native['state'] == 'paused' and not native['nativeSeeking'] and not native['error']
        assert row['nativeQueue'] == stages[0]['nativeQueue'] and native['track']['id'] == '1'
        assert all(native[key] == baseline[key] for key in ('playId', 'entryId', 'queueRevision', 'position'))
        assert row['vinylCommentsCalls'] == complete['vinylCommentsCalls']
    calls = complete['vinylCommentsCalls']
    assert calls == [
        {'path': COUNT, 'args': {'resourceType': 4, 'resourceIds': '["1"]'}},
        {'path': LATEST, 'args': dict(limit=60, offset=0, beforeTime=0, commentId='0',
                                     compareUserLocation=False, composeConcert=False, markReplied=False,
                                     forceFlatComment=False, showInner=False)},
    ], calls
    assert stages[-1]['copy'] == {'trusted': True, 'handled': True, 'hasClipboardData': True}
    assert driver.copied == EXPECTED and driver.rich_html_present is False
    report = {'ok': True, 'actualWebView': True, 'trustedX11': True, 'cohort': 't1', 'bodyOnly': True,
              'clipboard': driver.copied, 'richHtmlPresent': False, 'nativePosition': baseline['position'],
              'nativePlayIdUnchanged': True, 'nativeQueueUnchanged': True, 'wireReads': calls,
              'screenshots': len(LABELS), 'realMusicRequests': 0, 'accountWrites': 0,
              'physicalAudioOutputVerified': False}
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
