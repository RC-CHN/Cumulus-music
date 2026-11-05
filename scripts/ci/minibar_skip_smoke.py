"""Exercise the rendered main bottom bar through production Tauri and null libmpv."""
import argparse
import json
import os
from pathlib import Path
import sys

from window_manager import launch


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def check(directory, group, capture):
    before = read(directory / 'ux_01_before.json')
    after = read(directory / 'ux_02_after.json')
    complete = read(directory / 'complete.json')
    expected = '3' if group == 't1' else '1'
    assert complete['ok'] and complete['group'] == group and complete['target'] == expected, complete
    assert complete['stageCount'] == 2
    assert before['ids'] == after['ids'], (before, after)
    assert [item[0] for item in before['ids']] == ['1', '2', '3']
    for row, track in ((before, '1'), (after, expected)):
        player, dom = row['player'], row['dom']
        assert row['actual'] and row['group'] == group and row['online'] is True, row
        assert player['trackId'] == dom['trackId'] == track, row
        assert player['playId'] == dom['playId'] and player['playId'], row
        assert player['state'] == dom['state'] == 'playing' and player['error'] is None, row
        assert player['mode'] == 'single_loop' and 0.15 < player['position'] < 30, row
        assert player['duration'] == 30, row
        assert player['entryId'] == row['current'] == row['ids'][int(track) - 1][1], row
        assert dom['title'] == f'合成底栏歌曲 {track}' and dom['nextLabel'] == '下一首', row
        assert dom['nextEnabled'] and dom['unread'], row
    ack = after['acknowledgement']
    assert ack['trackId'] == expected and ack['playId'] == after['player']['playId'], ack
    assert ack['entryId'] == after['player']['entryId'] and ack['state'] == 'playing', ack
    if group == 't1':
        assert after['player']['playId'] != before['player']['playId']
    else:
        assert after['player']['playId'] == before['player']['playId']
        assert after['player']['queueRevision'] == before['player']['queueRevision']
        assert after['current'] == before['current']
        assert after['player']['position'] > before['player']['position'] + .05
    assert after['actions'] == [{
        'epoch': before['player']['epoch'], 'action': {
            'kind': 'step_mini_bar', 'direction': 'next', 'play_id': before['player']['playId'],
            'expected_queue_revision': before['player']['queueRevision'],
        },
    }], after['actions']
    auth = lambda trace: [row['id'] for row in trace if row['kind'] == 'authorization']
    assert auth(before['trace']) == ['1'], before['trace']
    assert auth(after['trace']) == (['1', '3'] if group == 't1' else ['1']), after['trace']
    assert after['trace'] == complete['trace'], complete['trace']
    assert [row for row in after['trace'] if row['kind'] == 'verified_assignment'] == [
        {'kind': 'verified_assignment', 'group': group},
    ], after['trace']
    assert sum(row['kind'] == 'count' for row in before['trace']) >= 1, before['trace']
    expected_requests = [{'kind': 'authorization', 'path': '/api/song/enhance/player/url/v1', 'id': '3'}]
    assert after['trace'][len(before['trace']):] == (expected_requests if group == 't1' else []), after['trace']
    locale = read(directory / 'locale.json')
    assert locale['numeric'] == 'C', locale
    assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
               for key in ('messages', 'characters')), locale
    screenshots = len(list(directory.glob('ux_*.png')))
    assert screenshots == (2 if capture else 0)
    return {'group': group, 'target': expected, 'stages': [before, after],
            'screenshots': screenshots, 'locale': locale}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'linux':
        parser.error('this isolated WebKitGTK proof runs on Linux')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    base.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.frontend_dist:
        base['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    binary = args.binary.resolve(strict=True)
    cases = {}
    for group in ('t1', 'c'):
        work = root / group
        work.mkdir()
        run = work / 'run'
        output = run / 'first'
        env = {**base, 'CUMULUS_PROOF_DIR': str(run), 'CUMULUS_MINIBAR_SKIP_GROUP': group}
        for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
            location = work / key
            location.mkdir(mode=0o700)
            env[key] = str(location)
        host = work / 'host.log'
        with host.open('x', encoding='utf-8') as log:
            status = launch(binary, env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), read(failed) if failed.exists() else None
        assert status == 0 and 'panicked at' not in host.read_text(encoding='utf-8'), host
        cases[group] = check(output, group, args.screenshots)
    report = {'actualWebView': True, 'decoder': 'libmpv', 'audioOutput': 'null',
              'transport': 'sealed_synthetic_verified_capability', 'cases': cases}
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps({group: case['target'] for group, case in cases.items()}))


if __name__ == '__main__':
    main()
