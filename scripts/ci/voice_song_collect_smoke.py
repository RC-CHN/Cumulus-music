"""Drive the sealed episode song group collector in the real Linux WebView."""

import argparse
import json
import os
import sqlite3
from pathlib import Path

from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from voice_song_download_smoke import persisted
from voice_song_multi_smoke import TrustedSongInput, suite_window_manager
from voice_songs_smoke import TRACKS, read
from window_manager import launch

STAGES = (
    'baseline', 'row_first', 'row_denied', 'row_no_play', 'row_last',
    'menu_1', 'collect_1', 'picker', 'cancelled', 'menu_2', 'collect_2',
    'pending', 'reconciled',
)
LIKED_STAGES = (*STAGES[:-2], 'choose_liked', 'pending', 'readback', 'reconciled')
LIST = '/api/user/playlist/v2'
STAR_LIST = '/api/user/playlist'
LIKED = '/api/song/like/get'
ACCOUNT = '/api/w/nuser/account/get'
DETAIL = '/api/v6/playlist/detail'
ADD = '/api/v1/playlist/manipulate/tracks'
IDS = ['1601', '1791', '1710']
ORDER = [1, 2, 3, 0]
OBSERVED_LIKES = ['701', '1710', '1601', '1791']
LIKED_READ = 'playerVoiceSongsCollectLikedDestination'
LIKED_SUBMIT = 'playerVoiceSongsCollectLiked'
OWNED_SUBMIT = 'playerVoiceSongsCollectExisting'
CREATE_ACTION = 'playerVoiceSongsCollectCreateAction'
CREATE = '/api/playlist/create'
CREATED_NAME = '合成新收藏歌单'


def commands(stage, method):
    return [row for row in stage['commands'] if row['name'] == method]


def verify_create(root, mode):
    output = root / 'run/first'
    paths = sorted(output.glob('ux_[0-9][0-9]_*.json'))
    stages = {path.stem.split('_', 2)[2]: read(path) for path in paths}
    ending = (('creation_pending', 'read_creation', 'created_ready', 'first_add', 'reconciled')
              if mode == 'recover' else ('creation_failed', 'closed_after_failure'))
    assert tuple(stages) == (*STAGES[:8], 'create_before_cancel', 'form_cancel', 'cancelled',
                             'menu_2', 'collect_2', 'create_open', 'form_ready', 'submit_create', *ending)
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['count'] == len(paths) and complete['creationMode'] == mode
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    base, last = stages['baseline'], stages[ending[-1]]
    player = base['player']
    assert (player['kind'], player['voiceId'], player['state']) == ('voice', '601', 'paused')
    assert player['position'] > .2 and not player['seeking'] and not player['error'], player
    assert all(stage['actual'] and stage['player'] == player and stage['queue'] == base['queue']
               for stage in stages.values())
    assert [entry['track']['voiceId'] for entry in base['queue']['tracks']] == ['601', '602']
    assert base['queue']['current'] == player['entryId']
    [first, second] = commands(last, 'playerVoiceSongsCollectPrepare')
    for call in (first, second):
        assert call['args'][1] == ORDER and call['result']['canCreate'] is True
        assert (call['result']['selectedCount'], call['result']['eligibleCount'], call['result']['omittedIndices']) == (4, 3, [2])
    assert first['args'][0] == second['args'][0]
    assert first['result']['selectionReceipt'] != second['result']['selectionReceipt']
    original = {**first['args'][0], 'selectionReceipt': first['result']['selectionReceipt']}
    active = {**second['args'][0], 'selectionReceipt': second['result']['selectionReceipt']}
    assert active['playId'] == player['playId'] and active['voiceId'] == '601'
    assert stages['picker']['creation']['row'] and stages['picker']['dom']['playlistIds'] == ['81']
    assert stages['form_cancel']['dom']['title'] == '创建歌单'
    assert stages['form_cancel']['creation'] == {
        'row': False, 'submit': False, 'name': '', 'nameFocused': True, 'private': False, 'canAdd': False,
    }
    assert stages['cancelled']['focusRow'] == '0' and last['focusRow'] == '0' and not last['dom']['picker']
    assert stages['form_ready']['creation']['name'] == CREATED_NAME and stages['form_ready']['creation']['private']
    assert stages['form_ready']['creation']['submit']
    actions = commands(last, CREATE_ACTION)
    assert actions[0]['args'] == [original, {'kind': 'cancel_prepared'}] and actions[0]['result'] == {'kind': 'cancelled'}
    assert actions[1]['args'] == [active, {'kind': 'create', 'name': CREATED_NAME, 'privatePlaylist': True}]
    assert commands(stages['form_cancel'], CREATE_ACTION) == []
    assert commands(stages['form_ready'], CREATE_ACTION) == actions[:1]
    if mode == 'recover':
        assert len(actions) == 4
        assert actions[2]['args'] == [active, {'kind': 'status', 'reconcile': True}]
        assert actions[3]['args'] == [active, {'kind': 'add_created'}]
        pending = actions[1]['result']['creation']
        assert (pending['createPhase'], pending['canAdd'], pending['add']) == ('uncertain', False, None)
        assert stages['creation_pending']['dom']['reconcile'] and not stages['creation_pending']['creation']['canAdd']
        verified = actions[2]['result']['creation']
        assert verified['playlistId'] == '91' and verified['canAdd'] and verified['add'] is None
        assert stages['created_ready']['creation']['canAdd'] and not stages['created_ready']['dom']['reconcile']
        assert pending['createOperationId'] == verified['createOperationId']
    else:
        assert len(actions) == 2
        failure = stages['creation_failed']
        assert failure['dom']['stage'] == 'failed' and failure['dom']['status'] == '歌单已创建，但所选歌曲未能收藏。'
        assert not failure['dom']['reconcile'] and not failure['creation']['canAdd']
    final = actions[-1]['result']['creation']
    assert final['createPhase'] == 'confirmed' and final['playlistId'] == '91' and not final['canAdd']
    assert final['selectionReceipt'] == active['selectionReceipt']
    assert final['createOperationId'] == actions[1]['result']['creation']['createOperationId']
    assert final['addFailed'] is (mode == 'add_false')
    add = final['add']
    assert (add['playlistId'], add['phase']) == ('91', 'confirmed') and add['operationId'] != final['createOperationId']
    assert add['selectionReceipt'] == active['selectionReceipt']
    assert [event['target'] for event in last['events'] if event['target'].startswith('create:')] == (
        ['create:submit', 'create:reconcile', 'create:add'] if mode == 'recover' else ['create:submit'])
    assert all(event['trusted'] for event in last['events'])
    listed = [LIST, {'uid': '2', 'offset': 0, 'limit': 10, 'sortType': 'default'}]
    host = lambda observed: [STAR_LIST, {'uid': '2', 'includeVideo': True, 'limit': 200, 'offset': 0}, observed]
    detail = lambda observed: [DETAIL, {'id': '91', 'n': 100000, 's': 0}, observed]
    write = [ADD, {'pid': '91', 'trackIds': json.dumps(IDS, separators=(',', ':')), 'op': 'add'}]
    wire = [host(False), listed, listed, host(False), [CREATE, {'uid': '2', 'name': CREATED_NAME, 'privacy': 10}]]
    if mode == 'recover':
        wire.append(host(False))
        assert stages['creation_pending']['voiceSongCollectTrace']['calls'] == wire
    wire.append(host(True))
    if mode == 'recover':
        assert stages['created_ready']['voiceSongCollectTrace']['calls'] == wire
    wire.extend([detail(False), write, detail(True)])
    if mode == 'recover':
        wire.append(host(True))  # Completed action refreshes the ordinary account playlist list.
        refresh = commands(last, 'playlists')[-1]['result']
        assert any(row['id'] == '91' and row['name'] == CREATED_NAME for row in refresh['value'])
    assert stages['cancelled']['voiceSongCollectTrace']['calls'] == [host(False), listed]
    assert complete['voiceSongCollectTrace']['calls'] == wire
    assert all(stage['voiceSongCollectTrace']['calls'] == wire[:len(stage['voiceSongCollectTrace']['calls'])]
               for stage in stages.values())
    records = persisted(root)
    assert set(records) == {('playlist-mutations', final['createOperationId']), ('playlist-mutations', add['operationId'])}
    saved = records['playlist-mutations', add['operationId']]
    assert saved['playlist_id'] == '91' and saved['command']['ids'] == IDS and saved['phase'] == 'confirmed'
    assert saved['codes']['main'] == (403 if mode == 'add_false' else 200)
    pngs = sorted(output.glob('ux_[0-9][0-9]_*.png'))
    assert len(pngs) == len(paths), pngs
    return {'ok': True, 'actualWebView': True, 'actualNullAudio': True, 'physicalAudioVerified': False,
            'case': mode, 'stages': len(paths), 'screenshots': len(pngs), 'syntheticPlaylistWrites': 2,
            'realMusicRequests': 0, 'realAccountWrites': 0}


def cached_likes(root):
    values = []
    for path in (root / 'run/state').rglob('core.sqlite3'):
        with sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            values.extend(json.loads(value) for (value,) in db.execute(
                "SELECT value FROM state WHERE owner='2' AND kind='resources' AND key='liked'",
            ))
    assert len(values) <= 1, values
    return values[0] if values else None


def verify_liked_wire(root, stages, complete, operation, listed):
    def detail(observed):
        return [DETAIL, {'id': '82', 'n': '100000', 's': '0'}, observed]

    def membership(observed):
        return [LIKED, {'uid': '2'}, observed]

    star = [STAR_LIST, {'uid': '2', 'offset': '0', 'limit': '1000'}, 'star']
    add = [ADD, {'pid': '82', 'trackIds': json.dumps(IDS, separators=(',', ':')), 'op': 'add'}, 'lost_ack']
    wire = [
        [ACCOUNT, {}, 'account'], [TRACKS, {'id': '601'}, 'episode'],
        star, detail('original'), star, detail('original'),
        star, detail('original'), membership('original'), add,
        star, detail('added'), membership('incomplete'),
        star, detail('added'), membership('complete'),
    ]
    for stage in (*stages.values(), complete):
        trace = stage['voiceSongGroupLikedTrace']
        assert trace['transport'] == 'MusicHttpClient' and trace['syntheticOnly'] is True
        actual = trace['signedCalls']
        assert actual == wire[:len(actual)], actual
        ordinary = stage['voiceSongCollectTrace']['calls']
        assert ordinary == [listed, listed][:len(ordinary)], ordinary
    for name, count in (
        ('baseline', 2), ('picker', 4), ('cancelled', 4), ('choose_liked', 6),
        ('pending', 13), ('readback', 13), ('reconciled', 16),
    ):
        assert stages[name]['voiceSongGroupLikedTrace']['signedCalls'] == wire[:count], name
    assert complete['voiceSongGroupLikedTrace']['signedCalls'] == wire
    assert complete['voiceSongCollectTrace']['calls'] == [listed, listed]

    pending = read(root / 'pending-journal.json')
    assert pending['operation'] == operation and pending['phase'] == 'uncertain', pending
    assert pending['tracks'] == [*IDS, '701'] and pending['likedIds'] == ['701', '1601'], pending
    assert not set(pending['cachedLikes'] or []).intersection(IDS), pending
    assert cached_likes(root) == OBSERVED_LIKES


def verify(root, liked=False):
    output = root / 'run/first'
    paths = sorted(output.glob('ux_[0-9][0-9]_*.json'))
    stages = {path.stem.split('_', 2)[2]: read(path) for path in paths}
    assert tuple(stages) == (LIKED_STAGES if liked else STAGES), list(stages)
    complete = read(output / 'complete.json')
    assert complete['ok'] and complete['count'] == len(paths)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    base, last = stages['baseline'], stages['reconciled']
    player = base['player']
    assert (player['kind'], player['voiceId'], player['state']) == ('voice', '601', 'paused')
    assert player['position'] > .2 and not player['seeking'] and not player['error'], player
    assert [entry['track']['voiceId'] for entry in base['queue']['tracks']] == ['601', '602']
    assert base['queue']['current'] == player['entryId']
    assert all(stage['actual'] and stage['player'] == player and stage['queue'] == base['queue']
               for stage in stages.values())
    assert all(path.with_suffix('.png').is_file() for path in paths)
    for path, stage in zip(paths, stages.values(), strict=True):
        if 'input' not in stage:
            continue
        spec = stage['input']
        event = read(path.with_name('input-' + path.name))
        assert event['request'] == spec
        assert {'target': spec['target'], 'trusted': True,
                'type': 'contextmenu' if spec['button'] == 3 else 'click',
                'button': 2 if spec['button'] == 3 else 0,
                'ctrl': spec['ctrl'], 'shift': False} in last['events'][len(stage['events']):]

    [song] = commands(last, 'playerVoiceSongs')
    epoch, play_id = song['args']
    card = song['result']
    assert play_id == player['playId'] == card['playId'] and card['voiceId'] == '601' and card['receipt']
    assert [track['id'] for track in card['tracks']] == ['1710', '1601', '1790', '1791']
    assert card['availability'][3] == {'onlineEligible': False, 'copyrightBlocked': True, 'groupDownload': 'omitted'}
    assert complete['voiceSongsTrace']['songReads'] == [[TRACKS, {'id': '601'}]]
    assert not complete['voiceSongsTrace']['likes']
    source = {'epoch': epoch, 'playId': play_id, 'voiceId': '601', 'cardReceipt': card['receipt']}
    prepares = commands(last, 'playerVoiceSongsCollectPrepare')
    lists = commands(last, 'playerVoiceSongsCollectDestinations')
    assert len(prepares) == len(lists) == 2
    scoped = []
    for prepare, listed in zip(prepares, lists, strict=True):
        assert prepare['args'] == [source, ORDER] and 'error' not in prepare
        plan = prepare['result']
        selection = plan['selectionReceipt']
        assert isinstance(selection, str) and len(selection) == 32
        assert plan == {'epoch': epoch, 'playId': play_id, 'selectionReceipt': selection,
                        'selectedCount': 4, 'eligibleCount': 3, 'omittedIndices': [2],
                        'usedFirstFallback': False, 'canCreate': False}, plan
        target = {**source, 'selectionReceipt': selection}
        assert listed['args'] == [target, 'default', 0] and 'error' not in listed
        assert [row['id'] for row in listed['result']['value']['playlists']] == ['81']
        assert listed['result']['value']['nextPage'] is None and listed['result']['epoch'] == epoch
        scoped.append(target)
    assert scoped[0] != scoped[1]
    submit = LIKED_SUBMIT if liked else OWNED_SUBMIT
    playlist = '82' if liked else '81'
    [attempt] = commands(last, submit)
    [status] = commands(last, 'playerVoiceSongsCollectStatus')
    chosen = '81'
    if liked:
        assert not commands(last, OWNED_SUBMIT)
        destinations = commands(last, LIKED_READ)
        assert len(destinations) == 2
        tickets = []
        for seen, target in zip(destinations, scoped, strict=True):
            assert seen['args'] == [target] and 'error' not in seen
            ticket = seen['result']['value']['destinationReceipt']
            assert isinstance(ticket, str) and len(ticket) == 32 and ticket != target['selectionReceipt']
            assert seen['result'] == {
                'epoch': epoch, 'value': {'playlistId': '82', 'destinationReceipt': ticket},
            }
            tickets.append(ticket)
        assert tickets[0] != tickets[1]
        chosen = tickets[1]
        assert stages['picker']['likedRow'] == stages['choose_liked']['likedRow'] == '我喜欢的音乐'
        assert all(stages[name]['likedRow'] is None for name in ('cancelled', 'pending', 'reconciled'))
    assert attempt['args'] == [scoped[1], chosen] and status['args'] == [scoped[1], True]
    assert 'error' not in attempt and 'error' not in status
    operation = attempt['result']['operationId']
    receipt = {'epoch': epoch, 'playId': play_id, 'selectionReceipt': scoped[1]['selectionReceipt'],
               'playlistId': playlist, 'operationId': operation}
    assert attempt['result'] == {**receipt, 'phase': 'uncertain'}
    assert status['result'] == {**receipt, 'phase': 'confirmed'}
    assert not any(commands(last, action) for action in (
        'playlistCollectPage', 'mutatePlaylist', 'playerVoiceSongsSelection', 'playerVoiceSongPlay'))
    assert not commands(stages['cancelled'], submit)
    assert not commands(stages['pending'], 'playerVoiceSongsCollectStatus')
    assert stages['picker']['dom'] == {
        'picker': True, 'stage': 'picking', 'title': '收藏到歌单',
        'summary': '已选 4 首，可收藏 3 首，跳过 1 首',
        'status': None, 'playlistIds': ['81'], 'reconcile': False,
    }, stages['picker']['dom']
    assert not stages['cancelled']['dom']['picker'] and stages['cancelled']['focusRow'] == '0'
    assert stages['pending']['dom']['picker'] and stages['pending']['dom']['stage'] == 'pending'
    assert stages['pending']['dom']['status'] == '收藏操作待核对' and stages['pending']['dom']['reconcile']
    assert not last['dom']['picker'] and last['focusRow'] == '0'

    listed = [LIST, {'uid': '2', 'offset': 0, 'limit': 10, 'sortType': 'default'}]
    if liked:
        verify_liked_wire(root, stages, complete, operation, listed)
    else:
        detail = [DETAIL, {'id': '81', 'n': 100000, 's': 0}]
        write = [ADD, {'pid': '81', 'trackIds': json.dumps(IDS, separators=(',', ':')), 'op': 'add'}]
        exact = [listed, listed, [*detail, False], write, [*detail, False], [*detail, True]]
        for stage in stages.values():
            actual = stage['voiceSongCollectTrace']['calls']
            assert actual == exact[:len(actual)], actual
        assert not base['voiceSongCollectTrace']['calls']
        assert stages['cancelled']['voiceSongCollectTrace']['calls'] == exact[:1]
        assert stages['pending']['voiceSongCollectTrace']['calls'] == exact[:-1]
        assert last['voiceSongCollectTrace']['calls'] == complete['voiceSongCollectTrace']['calls'] == exact
    records = persisted(root)
    assert set(records) == {('playlist-mutations', operation)}, records
    record = records['playlist-mutations', operation]
    assert record['phase'] == 'confirmed' and record['error_class'] == 'transport', record
    assert record['command']['ids'] == IDS and record['command']['id'] == playlist
    if liked:
        assert record['system_liked_group'] is True and record['playlist_id'] == '82'
        assert record['before']['playlist']['tracks'] == ['701']
        assert record['before']['liked_ids'] == ['701']
        assert record['observed']['playlist']['tracks'] == [*IDS, '701']
        assert record['observed']['liked_ids'] == OBSERVED_LIKES
    else:
        assert record['before']['tracks'] == ['701']
        assert record['observed']['tracks'] == [*IDS, '701']
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    return {'actualWebView': 'WebKitGTK', 'trustedX11Pointer': True, 'actualNullAudio': True,
            'stages': len(paths), 'screenshots': len(paths), 'selected': 4, 'eligible': 3,
            'syntheticAdds': 1, 'realMusicRequests': 0, 'realAccountWrites': 0,
            'physicalAudioVerified': False, **({
                'destination': 'system_liked', 'signedPcTransport': True, 'signedCalls': 16,
                'independentReadbackRequired': True, 'writeReplay': False,
            } if liked else {})}


def record_pending(root):
    [record] = persisted(root).values()
    assert record['system_liked_group'] is True
    value = {
        'operation': record['operation_id'], 'phase': record['phase'],
        'tracks': record['observed']['playlist']['tracks'],
        'likedIds': record['observed']['liked_ids'], 'cachedLikes': cached_likes(root),
    }
    (root / 'pending-journal.json').write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--liked', action='store_true', help='verify the signed system liked destination')
    mode.add_argument('--create', choices=('recover', 'add_false'), help='verify the sealed new destination')
    args = parser.parse_args()
    require_xvfb()
    if os.environ.get('CUMULUS_TEST_WINDOW_MANAGER') != 'openbox':
        parser.error('set CUMULUS_TEST_WINDOW_MANAGER=openbox on the dedicated Xvfb display')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_SONG_COLLECT_PROOF='1',
               CUMULUS_PODCAST_SCENE_HOME_PROOF='automatic', LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.liked:
        env['CUMULUS_VOICE_SONG_GROUP_LIKED_PROOF'] = '1'
    if args.create:
        env['CUMULUS_VOICE_SONG_COLLECT_CREATE_PROOF'] = args.create
    isolated_profile(env, root / 'application')
    output = root / 'run/first'
    with suite_window_manager(root), (root / 'host.log').open('x', encoding='utf-8') as log:
        driver = TrustedSongInput()

        def before_ack(path, payload, pid):
            if args.liked and path.stem.endswith('_pending'):
                record_pending(root)
            driver.at_capture(path, payload, pid)

        try:
            result = launch(args.binary.resolve(strict=True), env, log, output, True,
                            before_ack=before_ack, capture_glob='ux_[0-9][0-9]_*.json')
        finally:
            driver.close()
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, f'desktop native proof exited {result}'
    report = verify_create(root, args.create) if args.create else verify(root, args.liked)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
