"""Native Linux/Windows included-song rows, guarded likes, and queue controls."""
import argparse
import json
import os
from pathlib import Path
import sys

from podcast_scene_home_smoke import CHECK, DETAIL, read, requests
from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

BASELINE_CASES = ('cover', 'double')
CASES = {
    'dense': ('dense_first', 'dense_end', 'dense_home'),
    'cover': ('first_page', 'cover_single'),
    'double': ('first_page', 'selected_only', 'blocked', 'liked', 'unliked',
               'stale_rejected', 'context_next', 'double_order'),
}
TRACKS = '/api/v1/dj/program/tracks'
LIKE = '/api/song/like'
LIKED = '/api/song/like/get'
MEDIA = '/api/song/enhance/player/url/v1'
FIRST = ['1710', '1601', '1790', '1791']
SECOND = ['1811', '1810']


def pair_ids(queue):
    return [(entry['id'], entry['voiceId']) for entry in queue['entries']]


def verify_dense(stages, complete):
    labels = CASES['dense']
    assert tuple(stages) == labels and complete['ok'] and complete['mode'] == 'dense', complete
    assert complete['count'] == len(labels)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    initial, tail, back = (stages[label] for label in labels)
    native = complete['voiceSongsTrace']
    assert native['songReads'] == [[TRACKS, {'id': '601'}]] and not native['likes'] and not native['liked'], native
    assert '1601' in [id_ for _, _, id_ in native['mediaReads']]
    assert all(path == MEDIA and id_ in ('1601', '1602') for path, _, id_ in native['mediaReads']), native
    state = initial['player']
    assert state['kind'] == 'voice' and state['voiceId'] == '601' and state['trackId'] == '1601', state
    assert state['state'] == 'paused' and state['position'] > .2 and not state['seeking'] and not state['error'], state
    assert pair_ids(initial['queue']) == [('1601', '601'), ('1602', '602')], initial['queue']
    assert len(initial['reads']) == 1
    first_read = initial['reads'][0]
    assert first_read['args'] == [first_read['args'][0], state['playId']] and first_read['result']['receipt'], first_read
    assert [row['id'] for row in first_read['result']['tracks']] == [str(value) for value in range(2201, 2301)]
    for stage in (initial, tail, back):
        assert stage['actual'] and stage['dom']['shelf'] and not stage['plays'] and not stage['likes'], stage
        assert stage['player'] == state and stage['queue'] == initial['queue'] and stage['reads'] == initial['reads'], stage
        assert stage['voiceSongsTrace']['songReads'] == native['songReads'] and not stage['voiceSongsTrace']['likes'], stage
        geometry = stage['geometry']
        assert geometry['noInnerScroll'] and geometry['total'] == 100, geometry
        assert 0 < geometry['rendered'] <= geometry['maxRendered'] <= geometry['cap'] < 100, geometry
    assert [stage['dom']['focusedRow'] for stage in (initial, tail, back)] == ['0', '99', '0']
    assert initial['geometry']['firstVisible'] and back['geometry']['firstVisible'] and tail['geometry']['lastVisible']
    assert tail['geometry']['firstRendered'] > 0 and back['geometry']['lastRendered'] < 99
    assert tail['geometry']['top'] - initial['geometry']['top'] > 4000
    assert back['geometry']['top'] <= initial['geometry']['top'] + 1


def verify(case, stages, complete):
    if case == 'dense':
        verify_dense(stages, complete)
        return
    labels = CASES[case]
    assert tuple(stages) == labels and complete['ok'] and complete['mode'] == case, complete
    assert complete['count'] == len(labels)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    assert all(stage['actual'] and stage['mode'] == case and not stage['player']['error']
               for stage in stages.values()), stages
    initial = stages['first_page']
    first = initial['player']
    original_queue = initial['queue']
    assert first['kind'] == 'voice' and first['trackId'] == '1601' and first['voiceId'] == '601'
    assert first['state'] == 'paused' and first['position'] > .2 and not first['seeking'], first
    assert pair_ids(original_queue) == [('1601', '601'), ('1602', '602')], original_queue
    assert initial['dom']['shelf'] and '包含歌曲' in initial['dom']['title']
    assert len(initial['dom']['rows']) == 4 and not initial['plays'] and not initial['likes']
    assert initial['dom']['rows'][2]['coverDisabled'], initial['dom']
    assert '无专辑歌曲' not in initial['dom']['title']
    first_read = initial['reads'][0]
    epoch = first_read['args'][0]
    first_page = first_read['result']
    first_receipt = first_page['receipt']
    assert first_read['args'] == [epoch, first['playId']] and first_receipt
    assert first_page['playId'] == first['playId'] and first_page['voiceId'] == '601'
    assert [row['id'] for row in first_page['tracks']] == FIRST
    assert first_page['availability'] == [
        {'onlineEligible': True, 'copyrightBlocked': False, 'groupDownload': 'omitted'},
        {'onlineEligible': True, 'copyrightBlocked': False, 'groupDownload': 'omitted'},
        {'onlineEligible': False, 'copyrightBlocked': True, 'groupDownload': 'omitted'},
        {'onlineEligible': True, 'copyrightBlocked': True, 'groupDownload': 'omitted'},
    ]
    native = complete['voiceSongsTrace']
    assert stages[labels[-1]]['voiceSongsTrace']['songReads'] == native['songReads']
    voices = ['601'] if case == 'cover' else ['601', '602']
    assert native['songReads'] == [[TRACKS, {'id': id_}] for id_ in voices], native
    assert native['liked'] is False
    assert all(path == LIKED and body == {'uid': '2'} and ids in ([], [1710])
               for path, body, ids in native['likeReads']), native
    media_ids = [id_ for _, _, id_ in native['mediaReads']]
    assert all(path == MEDIA and body['level'] == 'standard' and
               str(json.loads(body['ids'])[0]) == id_ and len(json.loads(body['ids'])) == 1
               for path, body, id_ in native['mediaReads']), native
    scene_wire = complete['calls']
    check_ids = [str(row['voiceId']) for row in requests(scene_wire, CHECK)]
    details = [str(row['id']) for row in requests(scene_wire, DETAIL)]
    # Native prefetch may legitimately authorize the next queued voice before
    # a button is pressed. It may never become a request for the absent row.
    assert check_ids and details and check_ids[0] == details[0] == '601', scene_wire
    assert set(check_ids + details) <= {'601', '602'}, scene_wire
    if case == 'cover':
        last = stages['cover_single']
        player = last['player']
        assert player['kind'] == 'catalog' and player['trackId'] == '1601' and player['voiceId'] is None
        assert player['playId'] != first['playId'] and player['entryId'] != first['entryId']
        assert player['state'] == 'playing' and player['position'] > .2, player
        assert last['queue']['entries'][::2] == original_queue['entries']
        assert pair_ids(last['queue']) == [('1601', '601'), ('1601', None), ('1602', '602')]
        assert last['queue']['current'] == player['entryId'] and not last['dom']['shelf']
        assert [call['args'] for call in last['plays']] == [
            [epoch, first['playId'], first_receipt, {'kind': 'cover', 'index': 1}, 'standard'],
        ]
        assert not native['likes'] and not last['likes'], last
        assert set(media_ids) <= {'1601', '1602'} and media_ids.count('1601') == 2, native
        return
    for label in ('selected_only', 'blocked', 'liked', 'unliked'):
        stage = stages[label]
        assert stage['player'] == first and stage['queue'] == original_queue, stage
        assert not stage['plays'], stage
    assert stages['selected_only']['dom']['rows'][0]['selected'] == 'true'
    assert stages['blocked']['dom']['nextDisabled'] and stages['blocked']['dom']['playDisabled']
    assert stages['liked']['pressed'] == 'true' and stages['unliked']['pressed'] == 'false'
    assert not stages['blocked']['voiceSongsTrace']['likes']
    assert len(stages['liked']['voiceSongsTrace']['likes']) == 1
    assert native['likes'] == [[LIKE, {'trackId': '1710', 'userid': '2', 'like': liked}]
                               for liked in (True, False)], native
    assert [call['args'] for call in stages['unliked']['likes']] == [
        [epoch, first['playId'], first_receipt, 0, liked] for liked in (True, False)
    ]
    assert [call['result'] for call in stages['unliked']['likes']] == [
        {'epoch': epoch, 'value': {'id': '1710', 'phase': 'confirmed', 'observed': liked}}
        for liked in (True, False)
    ]
    readbacks = [ids for _, _, ids in native['likeReads']]
    assert [] in readbacks[:readbacks.index([1710])] and readbacks[-1] == [], readbacks
    retired = stages['stale_rejected']
    second = retired['player']
    second_page = retired['reads'][1]['result']
    second_receipt = second_page['receipt']
    assert retired['reject'] == {'play': 'stale', 'like': 'stale'} and not retired['plays'], retired
    assert second['kind'] == 'voice' and second['trackId'] == '1602' and second['voiceId'] == '602'
    assert second['state'] == 'paused' and second['position'] > .2 and not second['seeking'], second
    assert pair_ids(retired['queue']) == pair_ids(original_queue)
    assert retired['reads'][1]['args'] == [epoch, second['playId']] and second_receipt != first_receipt
    assert [row['id'] for row in second_page['tracks']] == SECOND and second_page['voiceId'] == '602'
    queued = stages['context_next']
    assert pair_ids(queued['queue']) == [('1601', '601'), ('1602', '602'), ('1811', None)]
    assert queued['queue']['entries'][:2] == retired['queue']['entries']
    for key in ('kind', 'trackId', 'voiceId', 'playId', 'entryId', 'state', 'position'):
        assert queued['player'][key] == second[key], queued
    assert queued['queue']['revision'] != retired['queue']['revision'] and queued['queue']['current'] == second['entryId']
    final = stages['double_order']
    assert [call['args'] for call in final['plays']] == [
        [epoch, second['playId'], second_receipt, {'kind': 'next_play', 'index': 0}, 'standard'],
        [epoch, second['playId'], second_receipt, {'kind': 'double_click', 'index': 1}, 'standard'],
    ]
    assert pair_ids(final['queue']) == [(id_, None) for id_ in SECOND], final
    assert final['queue']['current'] == final['player']['entryId'] == final['queue']['entries'][1]['entryId']
    assert final['player']['kind'] == 'catalog' and final['player']['playId'] != second['playId']
    assert final['player']['state'] == 'playing' and final['player']['position'] > .2 and not final['dom']['shelf']
    assert '602' in check_ids and '602' in details
    assert {'1601', '1602', '1810'} <= set(media_ids) <= {'1601', '1602', '1810', '1811'}, native


def run(args, root, case):
    root.mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_SONGS_PROOF=case,
               CUMULUS_PODCAST_SCENE_HOME_PROOF='automatic')
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime) + os.pathsep + env.get('PATH', '')
    if args.script:
        env['CUMULUS_UX_SCRIPT'] = str(args.script.resolve(strict=True))
    isolated_profile(env, root)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, (root / 'host.log').read_text(encoding='utf-8')
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    verify(case, stages, read(output / 'complete.json'))
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    pngs = list(output.glob('ux_*.png'))
    assert len(pngs) == (len(CASES[case]) if args.screenshots else 0), pngs
    return {'case': case, 'stages': len(stages), 'screenshots': len(pngs)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, help='explicit isolated dist override; default uses embedded assets')
    parser.add_argument('--runtime', type=Path, help='packaged Windows native library directory')
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--case', choices=('both', *CASES), default='both')
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--script', type=Path, help='explicit probe-only script for an existing exact binary')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'win32') or (args.screenshots and sys.platform != 'linux'):
        parser.error('use Linux X11 or Windows; screenshots require X11')
    if sys.platform == 'linux':
        require_xvfb()
    if args.runtime:
        args.runtime = args.runtime.resolve(strict=True)
        if not args.runtime.is_dir():
            parser.error('--runtime must name a directory')
    if sys.platform == 'win32' and (not args.runtime or not (args.runtime / 'libmpv-2.dll').is_file()):
        parser.error('Windows requires --runtime with the packaged libmpv-2.dll')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    requested = BASELINE_CASES if args.case == 'both' else (args.case,)
    cases = [run(args, root / case, case) for case in requested]
    report = {'actualWebView': True, 'actualNullAudio': True,
              'webViewRuntime': 'WebKitGTK' if sys.platform == 'linux' else 'WebView2',
              'cases': cases,
              'realMusicRequests': 0, 'realAccountWrites': 0, 'physicalAudioVerified': False}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
