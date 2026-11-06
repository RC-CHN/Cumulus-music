"""Drive the sealed episode song group collector in the real Linux WebView."""

import argparse
import json
import os
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
LIST = '/api/user/playlist/v2'
DETAIL = '/api/v6/playlist/detail'
ADD = '/api/v1/playlist/manipulate/tracks'
IDS = ['1601', '1791', '1710']
ORDER = [1, 2, 3, 0]


def commands(stage, method):
    return [row for row in stage['commands'] if row['name'] == method]


def verify(root):
    output = root / 'run/first'
    paths = sorted(output.glob('ux_[0-9][0-9]_*.json'))
    stages = {path.stem.split('_', 2)[2]: read(path) for path in paths}
    assert tuple(stages) == STAGES, list(stages)
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
    assert card['availability'][3] == {'onlineEligible': False, 'copyrightBlocked': True}
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
                        'usedFirstFallback': False}
        target = {**source, 'selectionReceipt': selection}
        assert listed['args'] == [target, 'default', 0] and 'error' not in listed
        assert [row['id'] for row in listed['result']['value']['playlists']] == ['81']
        assert listed['result']['value']['nextPage'] is None and listed['result']['epoch'] == epoch
        scoped.append(target)
    assert scoped[0] != scoped[1]
    [attempt] = commands(last, 'playerVoiceSongsCollectExisting')
    [status] = commands(last, 'playerVoiceSongsCollectStatus')
    assert attempt['args'] == [scoped[1], '81'] and status['args'] == [scoped[1], True]
    assert 'error' not in attempt and 'error' not in status
    operation = attempt['result']['operationId']
    receipt = {'epoch': epoch, 'playId': play_id, 'selectionReceipt': scoped[1]['selectionReceipt'],
               'playlistId': '81', 'operationId': operation}
    assert attempt['result'] == {**receipt, 'phase': 'uncertain'}
    assert status['result'] == {**receipt, 'phase': 'confirmed'}
    assert not any(commands(last, action) for action in (
        'playlistCollectPage', 'mutatePlaylist', 'playerVoiceSongsSelection', 'playerVoiceSongPlay'))
    assert not commands(stages['cancelled'], 'playerVoiceSongsCollectExisting')
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
    assert record['command']['ids'] == IDS and record['command']['id'] == '81'
    assert record['before']['tracks'] == ['701']
    assert record['observed']['tracks'] == [*IDS, '701']
    locale = read(output / 'locale.json')
    assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    return {'actualWebView': 'WebKitGTK', 'trustedX11Pointer': True, 'actualNullAudio': True,
            'stages': len(paths), 'screenshots': len(paths), 'selected': 4, 'eligible': 3,
            'syntheticAdds': 1, 'realMusicRequests': 0, 'realAccountWrites': 0,
            'physicalAudioVerified': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
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
    isolated_profile(env, root / 'application')
    output = root / 'run/first'
    with suite_window_manager(root), (root / 'host.log').open('x', encoding='utf-8') as log:
        driver = TrustedSongInput()
        try:
            result = launch(args.binary.resolve(strict=True), env, log, output, True,
                            before_ack=driver.at_capture, capture_glob='ux_[0-9][0-9]_*.json')
        finally:
            driver.close()
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, f'desktop native proof exited {result}'
    report = verify(root)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
