"""Lyric result layout, expansion and native clipboard via production IPC and an isolated in-memory backend."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


COMMENT_STAGES = (
    'lyric_comment_initial', 'lyric_comment_first', 'lyric_comment_duplicate',
    'lyric_comment_thread', 'lyric_comment_zero', 'lyric_comment_transport',
    'lyric_comment_type_hidden', 'lyric_comment_stale', 'lyric_comment_reopened',
)
GROUP_STAGES = (
    'lyric_group_initial', 'lyric_group_selected', 'lyric_group_blocked',
    'lyric_group_next', 'lyric_group_play', 'lyric_group_continuation',
    'lyric_group_restored', 'lyric_group_single_outsider', 'lyric_group_tab_reset',
)
COUNT_PATH = '/api/resource/commentInfo/list'
LYRIC_PATH = '/api/search/resource/lyric'
THREAD_PATH = '/api/v1/resource/comments/R_SO_4_3'


def verify_group(root, screenshots):
    first = root / 'first'
    read = lambda p: json.loads(p.read_text(encoding='utf-8'))
    complete = read(first / 'complete.json')
    names = [f'ux_{i:02}_{label}' for i, label in enumerate(GROUP_STAGES, 1)]
    assert complete['ok'] and complete['searchLyricGroupVerified'] and complete['stageCount'] == len(names)
    assert [p.stem for p in sorted(first.glob('ux_*.json'))] == names
    assert [p.stem for p in sorted(first.glob('ux_*.png'))] == (names if screenshots else [])
    rows = [read(first / f'{name}.json') for name in names]
    assert [row['label'] for row in rows] == list(GROUP_STAGES)
    for index in (1, 2, 3, 4, 6):
        assert rows[index]['selectedOrdinals'] == [0, 1, 2, 3]
    assert rows[1]['modifiedOutsiderGroupDisabled'] is True
    assert rows[7]['selectedOrdinals'] == [4] and len(rows[8]['selectedOrdinals']) <= 1

    def identity(row, ids, state=None, track=None):
        snapshot, queue = row['nativePlayer'], row['nativeQueue']
        assert queue['epoch'] == snapshot['epoch'] and queue['revision'] == snapshot['queueRevision']
        assert queue['current'] == snapshot['entryId']
        assert [entry['track']['id'] for entry in queue['tracks']] == ids
        assert len({entry['entryId'] for entry in queue['tracks']}) == len(ids)
        if state is not None:
            assert snapshot['state'] == state
        if track is not None:
            assert snapshot['track']['id'] == track
        return snapshot['epoch'], snapshot['playId'], snapshot['entryId']

    baseline = identity(rows[0], ['91', '92'], 'paused', '91')
    for row in rows[1:3]:
        assert identity(row, ['91', '92'], 'paused', '91') == baseline
        assert row['nativeQueue']['revision'] == rows[0]['nativeQueue']['revision']
    final_ids = ['91', '103', '101', '92']
    assert identity(rows[3], final_ids, 'paused', '91') == baseline
    assert identity(rows[4], final_ids, 'playing', '101') != baseline
    assert rows[4]['nativePlayer']['position'] > .1
    assert complete['finalQueue'] == final_ids
    operations = complete['operations']
    assert len(operations) == 3
    wanted = [
        dict(id='103', ordinal=2, right='playable'),
        dict(id='102', ordinal=1, right='copyright_blocked'),
        dict(id='101', ordinal=3, right='playable'),
    ]
    for index, operation in enumerate(operations[-2:]):
        assert operation == dict(operation=('next', 'play')[index], anchor=wanted[2], rows=wanted,
                                 hasPlayId=True, queueRevision=rows[(0, 3)[index]]['nativeQueue']['revision'])
    blocked_rows = [*wanted[:2], dict(wanted[2], ordinal=0)]
    assert operations[0] == dict(operation='next', anchor=wanted[1], rows=blocked_rows,
                                 hasPlayId=True, queueRevision=rows[0]['nativeQueue']['revision'])
    assert all(row['searchLyricGroupFixture']['rowCounts'] == [] for row in rows[:7])
    fixture = complete['searchLyricGroupFixture']
    assert fixture['rowCounts'] == [dict(source='Lyrics', args=dict(resourceType=4, resourceIds='["104"]'))]
    assert [record['args']['offset'] for record in fixture['lyrics']][:2] == [0, 10]
    assert all(record['path'] == LYRIC_PATH and not record['cached'] and
               record['args']['keyword'] == '合成歌词组选曲' for record in fixture['lyrics'])
    media = fixture['media']
    assert any(record['id'] == 101 and record['trialMode'] == 42 for record in media)
    assert all(record['id'] in (91, 92) or (record['id'] in (101, *range(103, 113)) and
               record['trialMode'] == 42) for record in media)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    report = dict(ok=True, actualWebView=True, nativeIpc=True, stageCount=len(rows),
                  exactClickedDuplicate=operations[-1]['anchor'], nativeQueue=final_ids,
                  selectedInGestureOrder=True, retainedThroughContinuation=True,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n',
                                     encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


def verify_comments(root, screenshots):
    first = root / 'first'
    read = lambda p: json.loads(p.read_text(encoding='utf-8'))
    complete = read(first / 'complete.json')
    expected = [f'ux_{i:02}_{label}' for i, label in enumerate(COMMENT_STAGES, 1)]
    assert complete['ok'] and complete['searchLyricCommentVerified'] and complete['focusRestored']
    assert complete['stageCount'] == len(expected)
    assert [p.stem for p in sorted(first.glob('ux_*.json'))] == expected
    assert [p.stem for p in sorted(first.glob('ux_*.png'))] == (expected if screenshots else [])
    rows = [read(first / f'{name}.json') for name in expected]
    assert [row['label'] for row in rows] == list(COMMENT_STAGES)
    initial, hit_a, hit_b, thread, zero, transport, hidden, stale, reopened = rows

    def hit(index, id):
        return dict(index=index, id=id, rowKey=f'lyric:{index}:{id}', excerpt=f'合成命中第{index + 1}行')

    assert initial['first'] == hit(0, '3') and initial['second'] == hit(1, '3')
    assert initial['duplicateReceiptsDistinct'] is True
    for row, index, id, via, label in (
        (hit_a, 0, '3', 'keyboard', '查看评论(36)'),
        (hit_b, 1, '3', 'button', '查看评论(42)'),
        (zero, 2, '4', 'context', '查看评论'),
        (transport, 3, '5', 'keyboard', '查看评论'),
        (hidden, 4, '7', 'button', None),
        (reopened, 5, '8', 'button', '查看评论(51)'),
    ):
        evidence = row['action']
        assert {key: evidence[key] for key in ('index', 'id', 'rowKey', 'excerpt')} == hit(index, id)
        assert (evidence['via'], evidence['label'], evidence['receiptMatched']) == (via, label, True)
        assert evidence['keys'][:2] == ['play', 'next']
        if label is None:
            assert 'comments' not in evidence['keys']
        else:
            assert evidence['keys'][2] == 'comments'
    assert (thread['clickedIndex'], thread['resource']) == (1, ['track', '3'])
    assert (stale['clickedIndex'], stale['heldReturned'], stale['oldMenuSuppressed'],
            stale['receiptMatched']) == (5, True, True, True)

    def count_args(id):
        return {'resourceType': 4, 'resourceIds': json.dumps([id])}

    counts = [dict(source='lyrics', path=COUNT_PATH, args=count_args(id), cached=False,
                   thread=wire_thread, count=count, kind=kind)
              for id, wire_thread, count, kind in (
                  ('3', 'R_SO_4_3', 36, 'first'), ('3', 'R_SO_4_3', 42, 'duplicate'),
                  ('4', 'R_SO_4_4', 0, 'zero'), ('5', None, None, 'transport'),
                  ('8', 'R_SO_4_8', 88, 'held'), ('8', 'R_SO_4_8', 51, 'reopened'),
              )]
    badge = [dict(path=COUNT_PATH, args=count_args('91'), cached=False)]
    fixture = complete['searchLyricCommentFixture']
    results = fixture['results']
    assert len(results) >= 2
    for i, call in enumerate(results):
        keyword = '合成歌词评论再查' if i == len(results) - 1 else '合成歌词评论'
        assert call == dict(path=LYRIC_PATH, args=dict(keyword=keyword, scene='normal', limit=10, offset=0),
                            cached=False)
    threads = fixture['threads']
    assert any(call['path'] == THREAD_PATH for call in threads)
    assert all(call['path'] in (THREAD_PATH, THREAD_PATH.replace('/comments/', '/hotcomments/'))
               and call['cached'] is False and call['args']['offset'] == 0 for call in threads)

    def queue_identity(row):
        player, queue = row['nativePlayer'], row['nativeQueue']
        assert player['state'] == 'paused' and player['track']['id'] == '91'
        assert all(isinstance(player[key], str) and player[key] for key in
                   ('epoch', 'playId', 'entryId', 'queueRevision'))
        assert (queue['epoch'], queue['current'], queue['revision']) == (
            player['epoch'], player['entryId'], player['queueRevision'])
        items = [(entry['entryId'], entry['track']['id']) for entry in queue['tracks']]
        assert [id for _, id in items] == ['91', '92', '3']
        assert len({entry for entry, _ in items}) == 3 and queue['current'] == items[0][0]
        return player['epoch'], player['playId'], player['entryId'], player['queueRevision'], items

    baseline = queue_identity(initial)
    assert all(row['playerBadge'] == '17' for row in rows)
    for i, (row, count) in enumerate(zip([*rows, complete], (0, 1, 2, 2, 3, 4, 4, 5, 6, 6), strict=True)):
        native = row['searchLyricCommentFixture']
        assert native['counts'] == counts[:count] and native['playerBadgeReads'] == badge
        assert native['threads'] == (threads if i >= 3 else [])
        assert not native['unexpected'] and queue_identity(row) == baseline
        assert (native['held'], native['released'], native['returned']) == (i >= 7,) * 3
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    report = dict(ok=True, actualWebView=True, nativeIpc=True, stageCount=len(rows), lyricResultReads=results,
                  distinctDuplicateOrdinals=True, countCalls=counts, exactThread=THREAD_PATH,
                  specialSongCountReads=0, staleCountSuppressed=True, nativeQueueUnchanged=True,
                  realMusicRequests=0, realAccountWrites=0)
    (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--waterfall', action='store_true')
    mode.add_argument('--comment-count', action='store_true')
    mode.add_argument('--group-play', action='store_true')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
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
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    script_name = 'search_lyrics_waterfall_ui_probe.js' if args.waterfall else 'search_lyrics_ui_probe.js'
    source = Path(__file__).resolve().parents[2] / 'desktop/examples' / script_name
    script = root / 'search-lyrics.js'
    if args.group_play:
        env['CUMULUS_SEARCH_LYRIC_GROUP_PROOF'] = '1'
        body = ('window.__cumulusSearchLyricGroupProof = true;\n'
                + source.with_name('search_lyric_group_ui_probe.js').read_text(encoding='utf-8') + '\n'
                + source.with_name('search_results_ui_probe.js').read_text(encoding='utf-8'))
    elif args.comment_count:
        env['CUMULUS_SEARCH_LYRIC_COMMENT_PROOF'] = '1'
        body = ('window.__cumulusSearchLyricCommentProof = true;\n'
                + source.with_name('search_lyric_comment_ui_probe.js').read_text(encoding='utf-8') + '\n'
                + source.with_name('search_results_ui_probe.js').read_text(encoding='utf-8'))
    else:
        body = source.read_text(encoding='utf-8')
    script.write_text(body, encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots,
                      acknowledge_receipts=args.comment_count or args.group_play)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    if args.group_play:
        verify_group(root, args.screenshots)
        return
    if args.comment_count:
        verify_comments(root, args.screenshots)
        return
    if args.waterfall:
        complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['lyricWaterfall'] and complete['stageCount'] == 4
        assert complete['offsets'] == [0, 10, 10, 20, 30, 40, 50, 60, 0, *range(10, 111, 10)]
        assert complete['nativePlaybackId'] == '58' and complete['duplicateHitsPreserved']
        terminal = json.loads((root / 'first/ux_02_short_batch_terminal.json').read_text(encoding='utf-8'))
        assert terminal['loaded'] == complete['terminalShortBatch'] == 63 and terminal['mounted'] < 28
        assert len(list((root / 'first').glob('ux_*.png'))) == (4 if args.screenshots else 0)
        print(json.dumps(dict(ok=True, actualWebView=True, lyricWaterfall=True)))
        return
    verify(root, args.screenshots)

def verify(root, capture):
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['lyricsLayout'] and complete['clipboardVerified'] and complete['clipboardFailureInjected']
    assert complete['stageCount'] == 42 and complete['syntheticSearchCalls'] >= 12
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert len(list((root / 'first').glob('ux_*.json'))) == complete['stageCount']
    if capture:
        assert len(list((root / 'first').glob('ux_*.png'))) == complete['stageCount']
    result = dict(complete, actualWebView=True,
                  screenshots=complete['stageCount'] if capture else 0,
                  physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: result[k] for k in ['ok', 'stageCount', 'screenshots', 'syntheticSearchCalls']}))


if __name__ == '__main__':
    main()
