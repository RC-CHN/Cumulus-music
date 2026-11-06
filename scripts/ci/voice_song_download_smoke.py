"""Run sealed SongCard downloads through the real desktop WebView and native task worker."""
import argparse
from hashlib import sha256
import json
import os
from pathlib import Path
import sqlite3
import sys

from podcast_scene_home_smoke import read
from queue_collection_smoke import isolated_profile
from vinyl_lyric_copy_smoke import require_xvfb
from window_manager import launch

CASES = {
    'liked': ('song_card', 'options', 'transferred'),
    'cancel': ('song_card', 'picker', 'cancelled'),
    'existing_rejected': ('song_card', 'picker', 'options', 'transferred'),
    'create_rejected': ('song_card', 'picker', 'create_form', 'add_rejected', 'closed'),
    'recover': ('song_card', 'picker', 'uncertain', 'reconciled', 'closed'),
}
TRACKS = '/api/v1/dj/program/tracks'
COLLECT = '/api/user/playlist/v2'
LIST = '/api/user/playlist'
DETAIL = '/api/v6/playlist/detail'
ADD = '/api/v1/playlist/manipulate/tracks'
CREATE = '/api/playlist/create'
META = '/api/v3/song/detail'
AUTH = '/api/song/enhance/download/url/v1'
NAME = '合成私密下载歌单'
MP3 = Path(__file__).resolve().parents[2] / 'core/tests/fixtures/tag_audio/seed.mp3'


def actions(stage, *names):
    return [row for row in stage['commands'] if row['name'] in names]


def action(stage, name, expected_args):
    [call] = actions(stage, name)
    assert call['args'] == expected_args and 'error' not in call, call
    return call['result']


def persisted(root):
    records = {}
    for path in (root / 'run/state').rglob('core.sqlite3'):
        # Profiles are created exclusively by this fixture. Read only the
        # synthetic task and journal namespaces, never cookies or credentials.
        with sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            for owner, kind, key, value in db.execute(
                "SELECT owner,kind,key,value FROM state WHERE kind IN ('download_tasks_v1','playlist-mutations')",
            ):
                assert owner == '2' and (kind, key) not in records
                records[kind, key] = json.loads(value)
    return records


def verify(case, root, stages, complete):
    labels = CASES[case]
    assert tuple(stages) == labels and complete['mode'] == case and complete['ok'], complete
    assert complete['count'] == len(labels)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    first, last = stages['song_card'], stages[labels[-1]]
    player = first['player']
    assert (player['kind'], player['trackId'], player['voiceId'], player['state']) == ('voice', '1601', '601', 'paused')
    assert player['position'] > .2 and not player['seeking'] and not player['error'], player
    assert [entry['track']['voiceId'] for entry in first['queue']['tracks']] == ['601', '602'], first['queue']
    assert first['queue']['current'] == player['entryId'], first
    assert all(stage['actual'] and stage['mode'] == case and
               stage['player'] == player and stage['queue'] == first['queue']
               for stage in stages.values()), stages
    assert all(stage['records'] == {'active': [], 'done': []}
               for label, stage in stages.items() if label != 'transferred'), stages
    [card_read] = actions(first, 'playerVoiceSongs')
    epoch, play_id = card_read['args']
    card = card_read['result']
    assert play_id == player['playId'] == card['playId'] and card['voiceId'] == '601' and card['receipt']
    assert [song['id'] for song in card['tracks']] == ['1710', '1601', '1790', '1791']
    assert complete['voiceSongsTrace']['songReads'] == [[TRACKS, {'id': '601'}]]
    assert complete['voiceSongsTrace']['likes'] == []
    target = {'epoch': epoch, 'playId': play_id, 'voiceId': '601', 'receipt': card['receipt'], 'index': 0}
    decision = 'ready' if case == 'liked' else 'playlist_required'
    assert action(last, 'playerVoiceSongDownloadPrepare', [target]) == {
        'epoch': epoch, 'playId': play_id, 'trackId': '1710', 'decision': decision,
    }
    wire = complete['voiceSongDownloadTrace']
    assert wire['likedReads'] > 0, wire
    collect = [COLLECT, {'uid': '2', 'offset': 0, 'limit': 10, 'sortType': 'default'}]
    list_create = [LIST, {'uid': '2', 'includeVideo': True, 'limit': 200, 'offset': 0}]

    def detail(pid, present=False):
        return [DETAIL, {'id': pid, 'n': 100000, 's': 0}, present]

    def add(pid):
        return [ADD, {'pid': pid, 'trackIds': '["1710"]', 'op': 'add'}]

    transfer = [
        [AUTH, {'id': '1710', 'level': 'standard', 'immerseType': 'c51'}],
        [META, {'c': '[{"id":"1710","v":0}]'}],
        ['media_get', {'offset': 0, 'bytes': MP3.stat().st_size}],
    ]
    expected = {
        'liked': transfer,
        'cancel': [collect],
        'existing_rejected': [collect, detail('81'), add('81'), detail('81'), *transfer],
        'create_rejected': [collect, list_create, [CREATE, {'uid': '2', 'name': NAME, 'privacy': 10}],
                            list_create, detail('84'), add('84'), detail('84')],
        'recover': [collect, detail('81'), add('81'), detail('81'), detail('81', True)],
    }[case]
    assert wire['calls'] == expected, wire
    for stage in stages.values():
        observed = stage['voiceSongDownloadTrace']['calls']
        assert observed == expected[:len(observed)], stage['voiceSongDownloadTrace']
    assert not first['voiceSongDownloadTrace']['calls'], first
    records = persisted(root)
    journal_ids = set()
    media_bytes = 0
    if case != 'liked':
        p = stages['picker']
        assert p['dom']['picker'] == 'pick' and p['dom']['title'] == '下载到歌单'
        assert p['dom']['pickerRows'] == ['创建新歌单', '下载到：合成已有下载歌单'], p
        assert p['voiceSongDownloadTrace']['calls'] == [collect], p
        collected = action(p, 'playlistCollectPage', [epoch, 'default', 0])
        assert [item['id'] for item in collected['value']['playlists']] == ['81']
    else:
        assert not actions(last, 'playlistCollectPage')
    if case in ('existing_rejected', 'recover'):
        choice = action(last, 'playerVoiceSongDownloadChoosePlaylist', [target, '81'])
        assert choice['trackId'] == '1710' and choice['playlistId'] == '81' and choice['playId'] == play_id
        journal_ids.add(choice['operationId'])
        row = records['playlist-mutations', choice['operationId']]
        if case == 'existing_rejected':
            assert choice['phase'] == 'rejected' and choice['addFailed'] and choice['continuation']
            assert row['phase'] == 'rejected' and row['error_class'] == 'service'
            assert stages['options']['dom']['status'] == '加入歌单失败；仍可继续设置下载。'
            assert stages['options']['voiceSongDownloadTrace']['calls'] == expected[:-3]
        else:
            before, after = stages['uncertain'], stages['reconciled']
            assert before['dom']['picker'] == 'recover' and not before['dom']['options']
            assert before['voiceSongDownloadTrace']['calls'] == expected[:-1]
            assert choice['phase'] == 'uncertain' and not choice['continuation'] and not choice['addFailed']
            check = action(last, 'playerVoiceSongDownloadPickerStatus', [target, True])
            assert check['operationId'] == choice['operationId'] and check['phase'] == 'confirmed'
            assert check['continuation'] and row['phase'] == 'confirmed'
            assert after['dom']['options'] and after['voiceSongDownloadTrace']['calls'] == expected
    elif case == 'create_rejected':
        made = action(last, 'playerVoiceSongDownloadCreatePlaylist', [target, NAME, True])
        a = made['add']
        assert made['createPhase'] == 'confirmed' and made['playlistId'] == a['playlistId'] == '84'
        assert a['trackId'] == '1710' and a['phase'] == 'rejected' and a['addFailed'] and not a['continuation']
        assert records['playlist-mutations', made['createOperationId']]['phase'] == 'confirmed'
        assert records['playlist-mutations', a['operationId']]['phase'] == 'rejected'
        journal_ids.update((made['createOperationId'], a['operationId']))
        assert stages['add_rejected']['dom']['picker'] == 'notice'
        assert not any(stage['dom']['options'] for stage in stages.values())
    else:
        assert not actions(last, 'playerVoiceSongDownloadChoosePlaylist', 'playerVoiceSongDownloadCreatePlaylist')
    assert {key for (kind, key) in records if kind == 'playlist-mutations'} == journal_ids
    assert not actions(last, 'playerVoiceSongDownloadCreationStatus', 'playerVoiceSongDownloadAddCreatedPlaylist')
    if case != 'recover':
        assert not actions(last, 'playerVoiceSongDownloadPickerStatus')
    configure = {'kind': 'configure', 'root': str(root / 'run/download-output'), 'path_type': 0, 'file_type': 1}
    options = {'quality': 'standard', 'tags': False, 'cover': False, 'lyrics': False, 'container': 'audio'}
    if case in ('liked', 'existing_rejected'):
        assert stages['options']['dom']['options'] and stages['options']['dom']['title'] == '下载歌曲'
        assert last['dom']['downloads'] and not last['dom']['vinyl'] and not last['records']['active']
        [task] = last['records']['done']
        assert task['trackId'] == '1710' and task['source'] == 'song' and task['status'] == 'complete'
        assert task['quality'] == 'standard' and task['encrypted'] is False
        assert task['bytes'] == MP3.stat().st_size
        path = Path(task['path']).resolve(strict=True)
        assert path.parent == Path(configure['root']).resolve(strict=True) and path.suffix == '.mp3'
        payload = path.read_bytes()
        assert payload == MP3.read_bytes()
        media_bytes = len(payload)
        record = records['download_tasks_v1', task['id']]
        assert record['song_id'] == '1710' and record['origin']['kind'] == 'song'
        assert record['status'] == 'complete' and record['authorization_count'] == 1
        assert {key for (kind, key) in records if kind == 'download_tasks_v1'} == {task['id']}
        assert action(last, 'downloadCommand', [epoch, configure])['failed'] == []
        if case == 'liked':
            accepted = action(last, 'playerVoiceSongDownloadStart', [target, options])
            assert not actions(last, 'playerVoiceSongDownloadStartFromPlaylist')
        else:
            accepted = action(last, 'playerVoiceSongDownloadStartFromPlaylist',
                              [target, choice['continuation'], options])
            assert not actions(last, 'playerVoiceSongDownloadStart')
        assert accepted['accepted'] == [task['id']] and accepted['failed'] == []
    else:
        assert not actions(last, 'playerVoiceSongDownloadStart', 'playerVoiceSongDownloadStartFromPlaylist',
                           'downloadCommand')
        assert last['dom']['vinyl'] and last['dom']['focus'] == '0' and not last['dom']['picker']
        assert not any(kind == 'download_tasks_v1' for kind, _ in records)
        assert not list((root / 'run/download-output').iterdir())
    return {'syntheticCalls': len(expected), 'journalEntries': len(journal_ids), 'mediaBytes': media_bytes,
            'mediaSha256': sha256(MP3.read_bytes()).hexdigest() if media_bytes else None}


def run(args, root, case):
    root.mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_SONG_DOWNLOAD_PROOF=case,
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
    evidence = verify(case, root, stages, read(output / 'complete.json'))
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    pngs = list(output.glob('ux_*.png'))
    assert len(pngs) == (len(CASES[case]) if args.screenshots else 0), pngs
    return {'case': case, 'stages': len(stages), 'screenshots': len(pngs), **evidence}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path, help='packaged Windows runtime with libmpv-2.dll')
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--case', choices=('all', *CASES), default='all')
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--script', type=Path, help='override only the embedded synthetic driver')
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
    requested = CASES if args.case == 'all' else (args.case,)
    cases = [run(args, root / case, case) for case in requested]
    report = {'actualWebView': True, 'actualNullAudio': True,
              'webViewRuntime': 'WebKitGTK' if sys.platform == 'linux' else 'WebView2',
              'cases': cases, 'realMusicRequests': 0, 'realAccountWrites': 0, 'physicalAudioVerified': False}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
