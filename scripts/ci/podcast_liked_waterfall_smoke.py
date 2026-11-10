"""Prove liked-programme waterfall, exact Retry, and POP in the Linux WebView."""

import argparse
import json
import os
from pathlib import Path
import sqlite3
import time

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

LIST = '/api/content/my/liked/voice'
COUNT = '/api/content/my/liked/voice/count'
COMMENTS = '/api/v1/resource/comments/A_DJ_1_1122'
DETAIL = '/api/dj/program/detail'
NAMES = ('library', 'first_page', 'tail_held', 'tail_failed', 'tail_retried',
         'before_comments', 'comments', 'back', 'menu', 'menu_back', 'final_page')


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def capture_checkpoint(database, path, receipts):
    if path.stem not in ('ux_07_comments', 'ux_08_back', 'ux_10_menu_back'):
        return
    deadline = time.monotonic() + 2
    while True:
        with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as store:
            cached = store.execute(
                "SELECT value FROM state WHERE kind='podcast-liked-prefix-v1' AND key='prefix'"
            ).fetchone()
        if cached or path.stem != 'ux_07_comments' or time.monotonic() >= deadline:
            break
        # The comments IPC can complete while the independent Suspend commits.
        time.sleep(.04)
    saved = json.loads(cached[0]) if cached else None
    receipts[path.stem] = None if saved is None else {
        'rows': len(saved['rows']), 'count': saved['count'], 'cursor': saved['cursor'],
    }


def verify(output):
    names = [f'ux_{i:02}_{name}' for i, name in enumerate(NAMES, 1)]
    for suffix in ('json', 'png'):
        assert sorted(path.stem for path in output.glob(f'ux_*.{suffix}')) == names
    stages = dict(zip(NAMES, (read(output / f'{name}.json') for name in names), strict=True))
    complete = read(output / 'complete.json')
    assert complete['ok'] and all(row['actual'] for row in stages.values())
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C'
    assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
               for key in ('messages', 'characters')), locale

    def requests(stage, path):
        return [body for endpoint, body, _ in stage['likedWaterfallTrace'] if endpoint == path]

    cursors = (0, '1700000000900', '1700000000900', 1700000000863)
    list_requests = [{'limit': 100, 'likeTime': cursor} for cursor in cursors]
    for stage, total in zip(stages.values(), (0, 1, 2, 2, 3, 3, 3, 3, 3, 3, 4), strict=True):
        assert requests(stage, LIST) == list_requests[:total], stage
    assert requests(complete, LIST) == list_requests
    assert requests(stages['library'], COUNT) == [{}]
    before = stages['before_comments']
    count_requests = requests(before, COUNT)
    assert count_requests in ([{}], [{}, {}])
    for stage in (stages['comments'], stages['back'], stages['menu'], stages['menu_back'],
                  stages['final_page'], complete):
        assert requests(stage, COUNT) == count_requests, stage
    assert all(cache is False for path, _, cache in complete['likedWaterfallTrace']
               if path in (LIST, COUNT))

    loaded = {'first_page': 100, 'tail_held': 100, 'tail_failed': 100, 'tail_retried': 137,
              'before_comments': 137, 'back': 137, 'menu': 137, 'menu_back': 137, 'final_page': 145}
    for name, expected in loaded.items():
        row = stages[name]
        assert row['loaded'] == expected and row['count'] == 145, row
        assert 0 < row['mounted'] == len(row['ids']) == len(row['indices']) < 30, row
        assert row['ids'] == [str(1001 + index) for index in row['indices']]
        assert row['indices'] == list(range(row['indices'][0], row['indices'][-1] + 1))
    assert stages['first_page']['ids'][0] == '1001'
    assert stages['tail_held']['ids'][-1] == stages['tail_failed']['ids'][-1] == '1100'
    assert stages['tail_failed']['retry'] == '重试'
    assert '1101' in stages['tail_retried']['ids'] and stages['tail_retried']['ids'].count('1100') == 1
    after = stages['back']
    assert before['voice'] == stages['comments']['voice'] == after['voice'] == '1122'
    assert '1122' in before['visibleIds'] and before['visibleIds'] == after['visibleIds']
    assert before['scrollTop'] > 1000 and abs(before['scrollTop'] - after['scrollTop']) <= 2
    assert abs(before['screenY'] - after['screenY']) <= 2
    assert after['commentFocused'] is True
    menu_back = stages['menu_back']
    assert stages['menu']['voice'] == menu_back['voice'] == '1122'
    assert before['visibleIds'] == menu_back['visibleIds']
    assert abs(before['scrollTop'] - menu_back['scrollTop']) <= 2
    assert abs(before['screenY'] - menu_back['screenY']) <= 2
    assert menu_back['moreFocused'] is True
    assert stages['final_page']['ids'][-1] == '1145'
    assert requests(complete, COMMENTS) and not requests(before, COMMENTS)
    assert requests(complete, DETAIL) == [{'id': '1122'}, {'id': '1122'}]
    assert not any(path in {'/api/dj/play/url/check', '/api/song/enhance/player/url/v1'}
                   for path, _, _ in complete['likedWaterfallTrace'])
    return {'ok': True, 'actualWebView': True, 'screenshots': len(names), 'loadedRows': 145,
            'maxMountedRows': max(stages[name]['mounted'] for name in loaded),
            'listCursors': list(cursors), 'countRequests': len(count_requests),
            'restoredScrollTop': after['scrollTop'], 'commentFocusRestored': True,
            'menuFocusRestored': True, 'realMusicRequests': 0, 'realAccountWrites': 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    require_xvfb()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    isolated_profile(env, root)
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/podcast_liked_waterfall_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_PROOF_CAPTURE_ACK='1',
               CUMULUS_PODCAST_LIKED_WATERFALL_PROOF='1', CUMULUS_UX_SCRIPT=str(script),
               LC_ALL='en_US.UTF-8')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    output = root / 'run/first'
    checkpoint_receipts = {}
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(
            args.binary.resolve(strict=True), env, log, output, True,
            before_ack=lambda path, _, _pid: capture_checkpoint(
                root / 'run/state/core.sqlite3', path, checkpoint_receipts),
        )
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'WebView exited {code}; see {root / "host.log"}'
    report = verify(output)
    assert checkpoint_receipts == {
        'ux_07_comments': {'rows': 137, 'count': 145, 'cursor': 1700000000863},
        'ux_08_back': None,
        'ux_10_menu_back': None,
    }, checkpoint_receipts
    report['nativeCheckpoints'] = checkpoint_receipts
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
