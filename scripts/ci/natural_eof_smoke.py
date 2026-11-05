"""Verify natural EOF in the actual PlayerBar, Tauri, SQLite, and null libmpv."""
import argparse
import json
import os
from pathlib import Path
import sys

from minibar_skip_smoke import read
from window_manager import launch


CASES = {
    't1-skip': ('t1', '3', 'playing'),
    'control': ('c', '1', 'ended'),
    't1-tail': ('t1', '1', 'idle'),
}
WIRE = '/api/song/enhance/player/url/v1'


def check(directory, case, capture):
    group, target, state = CASES[case]
    before = read(directory / 'ux_01_before.json')
    after = read(directory / 'ux_02_after.json')
    complete = read(directory / 'complete.json')
    assert complete['ok'] and complete['case'] == case and complete['group'] == group
    assert complete['target'] == target and complete['stageCount'] == 2, complete
    assert before['ids'] == after['ids'] and [row[0] for row in before['ids']] == ['1', '2', '3']
    for row, track, status in ((before, '1', 'paused'), (after, target, state)):
        player, dom = row['player'], row['dom']
        assert row['case'] == case and row['group'] == group and row['actual'] and row['online']
        assert player['trackId'] == dom['trackId'] == track, row
        assert player['state'] == dom['state'] == status and player['error'] is None, row
        assert player['playId'] == dom['playId'] and player['mode'] == 'sequential', row
        assert player['entryId'] == row['current'] == row['ids'][int(track) - 1][1], row
        assert dom['unread'] and dom['title'] == f'合成自然结束歌曲 {track}', dom
        assert dom['primaryLabel'] == ('暂停' if status == 'playing' else '播放'), dom
    previous, current = before['player'], after['player']
    assert .2 < previous['position'] < 7.5 and previous['duration'] == 8
    assert previous['timelineMode'] == 'full' and previous['playId']
    assert [action['action']['paused'] for action in after['actions']] == [True, False]
    assert after['actions'] == [
        {'epoch': previous['epoch'], 'action': {
            'kind': 'pause_for', 'play_id': previous['playId'],
            'expected_queue_revision': previous['queueRevision'], 'paused': paused,
        }}
        for paused in (True, False)
    ], after['actions']
    assert [reply['state'] for reply in after['replies']] == ['paused', 'playing']
    assert all(reply['playId'] == previous['playId'] and reply['entryId'] == before['current']
               for reply in after['replies'])

    prefix, trace = before['trace'], after['trace']
    assert trace[:len(prefix)] == prefix and complete['trace'] == trace, trace
    assert [row for row in trace if row['kind'] == 'verified_assignment'] == [
        {'kind': 'verified_assignment', 'group': group},
    ], trace
    assert any(row['kind'] == 'count' and row['path'] == '/api/pl/count' for row in prefix), prefix
    assert all(row.get('path') != '/api/v3/song/detail' for row in trace), trace
    authorization = [row for row in trace if row['kind'] == 'authorization']
    expected_ids = ['1', '2', '3'] if case == 't1-skip' else ['1', '2']
    assert authorization == [
        {'kind': 'authorization', 'path': WIRE, 'id': item} for item in expected_ids
    ], trace
    assert authorization[:2] == [row for row in prefix if row['kind'] == 'authorization'], prefix
    loads = [row for row in trace if row['kind'] == 'native_load']
    assert [(row['id'], row['duration']) for row in loads] == (
        [('1', 8), ('3', 30)] if case == 't1-skip' else [('1', 8)]
    ), trace
    old = loads[0]['entry']
    events = [row for row in trace if row['kind'] == 'native_eof']
    assert events == [{'kind': 'native_eof', 'entry': old, 'error': 0}], trace
    assert events[0] not in prefix and trace.index(loads[0]) < trace.index(events[0]), trace
    assert trace.index(authorization[1]) < trace.index(events[0]), trace
    assert [row for row in trace if row['kind'] == 'native_output'] == [
        {'kind': 'native_output', 'entry': row['entry'], 'output': 'null'} for row in loads
    ], trace

    def old_checkpoint(value):
        assert value['entry_id'] == before['current'] and value['state'] == 'ended'
        assert value['seconds'] == 8 and value['paused'] is False, value

    stops = [row for row in trace if row['kind'] == 'native_stop']
    if case == 't1-skip':
        assert .2 < current['position'] < 30 and current['duration'] == 30, current
        assert current['playId'] and current['playId'] != previous['playId']
        assert current['queueRevision'] != previous['queueRevision'] and current['timelineMode'] == 'full'
        assert loads[1]['entry'] != old and trace.index(events[0]) < trace.index(authorization[2])
        assert trace.index(authorization[2]) < trace.index(loads[1]), trace
        assert stops == [{'kind': 'native_stop', 'entry': old}], trace
        old_checkpoint(loads[1]['positionBeforeLoad'])
    else:
        assert current['position'] == after['dom']['position'] == 8, after
        assert after['dom']['elapsed'] == '00:08', after['dom']
        assert current['queueRevision'] == previous['queueRevision']
        old_checkpoint(after['durablePosition'])
        old_checkpoint(complete['durablePosition'])
        if case == 'control':
            assert current['playId'] == previous['playId'] and current['duration'] == 8
            assert not stops, trace
        else:
            assert current['playId'] is None and current['duration'] is None
            assert stops == [{'kind': 'native_stop', 'entry': old}]
            assert trace.index(stops[0]) > trace.index(events[0]), trace

    locale = read(directory / 'locale.json')
    assert locale['numeric'] == 'C', locale
    assert all(locale[key].casefold().replace('-', '') == 'en_us.utf8'
               for key in ('messages', 'characters')), locale
    images = sorted(directory.glob('ux_*.png'))
    assert len(images) == (2 if capture else 0), images
    if capture:
        for image in images:
            paint = read(image.with_name('paint-' + image.stem + '.json'))
            assert len(paint['samples']) == 2 and all(sample['ok'] for sample in paint['samples']), paint
    return {'case': case, 'group': group, 'target': target, 'state': state,
            'stages': [before, after], 'locale': locale, 'screenshots': len(images)}


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
    for case in CASES:
        work = root / case
        work.mkdir()
        run = work / 'run'
        output = run / 'first'
        env = {**base, 'CUMULUS_PROOF_DIR': str(run), 'CUMULUS_ORDINARY_EOF_CASE': case}
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
        cases[case] = check(output, case, args.screenshots)
    report = {'actualWebView': True, 'decoder': 'libmpv', 'audioOutput': 'null',
              'transport': 'sealed_synthetic_verified_capability', 'cases': cases}
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                      encoding='utf-8')
    print(json.dumps({case: outcome['state'] for case, outcome in cases.items()}))


if __name__ == '__main__':
    main()
