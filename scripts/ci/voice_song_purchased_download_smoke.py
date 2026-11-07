"""Exercise purchased SongCard quality, owned destination and real local transfer in a sealed WebView."""
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
from voice_song_download_smoke import ADD, AUTH, COLLECT, DETAIL, META, MP3, TRACKS, persisted
from window_manager import launch

PREFS = {'quality': 'standard', 'tags': False, 'cover': False, 'lyrics': False, 'container': 'audio'}
OFFER = ['jymaster', 'hires', 'lossless', 'exhigh', 'standard']
NO_DIRECTORY = 'purchased_no_directory'
DIRECTORY_SETTINGS = 'purchased_directory_settings'
QUALITY_GUIDE = 'purchased_quality_guide'
STANDARD_GUIDE = 'https://y.music.163.com/m/at/648fba3cbe7c73d87cbc2e81'
LABELS = {
    'purchased_liked': ('song_card', 'quality', 'chosen', 'transferred'),
    'purchased_cancel': ('song_card', 'quality', 'chosen', 'cancelled'),
    'purchased_existing': ('song_card', 'quality', 'chosen', 'picker', 'transferred'),
    NO_DIRECTORY: ('song_card', 'quality', 'chosen', 'directory', 'directory_cancelled'),
    DIRECTORY_SETTINGS: ('song_card', 'quality', 'chosen', 'directory', 'settings', 'settings_closed'),
    QUALITY_GUIDE: ('song_card', 'quality', 'chosen', 'guide_opened', 'cancelled'),
}


def action(stage, name):
    [call] = [call for call in stage['commands'] if call['name'] == name]
    assert 'error' not in call, call
    return call


def purchased(stage):
    calls = [call for call in stage['commands'] if call['name'] == 'playerVoiceSongPurchasedDownload']
    assert all('error' not in call for call in calls), calls
    return calls


def assert_saved_directory_settings(root):
    rows = []
    for path in (root / 'run/state').rglob('core.sqlite3'):
        # Only local download settings from the synthetic profile.
        with sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True) as db:
            rows.extend(db.execute("SELECT owner,kind,key,value FROM state WHERE kind IN "
                                   "('download_directory_v1','download_preferences_v1')"))
    [(owner, kind, key, value)] = rows
    assert (owner, kind, key) == ('2', 'download_preferences_v1', 'options'), rows
    assert json.loads(value) == PREFS, rows


def verify(case, root, stages, complete):
    labels = LABELS[case]
    assert tuple(stages) == labels and complete['mode'] == case and complete['ok'], complete
    assert complete['count'] == len(labels)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['physicalAudioVerified'] is False
    first, last = stages['song_card'], stages[labels[-1]]
    player = first['player']
    assert (player['kind'], player['trackId'], player['voiceId'], player['state']) == ('voice', '1601', '601', 'paused')
    assert player['position'] > .2 and not player['seeking'] and not player['error'], player
    assert [entry['track']['voiceId'] for entry in first['queue']['tracks']] == ['601', '602']
    assert first['queue']['current'] == player['entryId']
    for label, stage in stages.items():
        downloads = case == DIRECTORY_SETTINGS and label in ('settings', 'settings_closed')
        assert (stage['actual'] and stage['mode'] == case and stage['player'] == player and
                stage['queue'] == first['queue'] and stage['dom']['vinyl'] is not downloads and
                stage['dom']['downloads'] is downloads and stage['dom']['settings'] is (label == 'settings') and
                not stage['dom']['optionsEver'] and stage['records']['preferences'] == PREFS), stage
    assert all(not stage['records']['active'] and not stage['records']['done']
               for label, stage in stages.items() if label != 'transferred')
    card = action(first, 'playerVoiceSongs')
    epoch, play_id = card['args']
    page = card['result']
    assert page['voiceId'] == '601' and play_id == player['playId'] == page['playId'] and page['receipt']
    row = page['tracks'][0]
    assert row['id'] == '1710' and row['fee'] == 0, row
    # The fixture sets privilege.fee, not the public song's top-level fee.
    # Only the native purchase plan and transport can observe that privilege.
    assert not {'privilege', 'payed', 'maxbr', 'flag'}.intersection(row), row
    assert complete['voiceSongsTrace']['songReads'] == [[TRACKS, {'id': '601'}]]
    assert complete['voiceSongsTrace']['likes'] == []
    assert all(row[2] in ('1601', '1602') for row in complete['voiceSongsTrace']['mediaReads'])
    target = {'epoch': epoch, 'playId': play_id, 'voiceId': '601', 'receipt': page['receipt'], 'index': 0}
    ordinary = action(last, 'playerVoiceSongDownloadPrepare')
    assert ordinary['args'] == [target]
    assert ordinary['result'] == {'epoch': epoch, 'playId': play_id, 'trackId': '1710',
                                  'decision': 'extended_rights_unsupported'}
    assert not any(call['name'] in (
        'downloadCommand', 'pickDownloadDirectory', 'playerVoiceSongDownloadStart', 'playerVoiceSongDownloadStartFromPlaylist',
        'playerVoiceSongDownloadChoosePlaylist', 'playerVoiceSongDownloadCreatePlaylist',
    ) for call in last['commands']), last
    proof = purchased(last)
    expected_quality = 'lossless' if case == 'purchased_existing' else 'exhigh'
    assert not purchased(first)
    assert proof[0]['args'] == [target, {'kind': 'prepare'}]
    assert proof[0]['result'] == {
        'kind': 'plan', 'plan': {'epoch': epoch, 'playId': play_id, 'trackId': '1710',
                                 'decision': 'purchased_quality_required', 'purchasedQualities': OFFER},
    }
    for label, quality in (('quality', OFFER[0]), ('chosen', expected_quality)):
        stage = stages[label]
        assert stage['dom']['title'] == '选择下载音质' and stage['dom']['qualityModal']
        assert stage['dom']['qualityRows'] == OFFER and stage['dom']['quality'] == quality
        assert purchased(stage) == proof[:1] and not stage['voiceSongDownloadTrace']['calls'], stage
    assert not last['dom']['qualityModal'] and not last['dom']['picker'] and not last['dom']['directory']
    assert last['dom']['notice'] == ('已添加到下载' if labels[-1] == 'transferred' else None), last['dom']
    wire = complete['voiceSongDownloadTrace']
    assert wire['likedReads'] > 0
    records = persisted(root)
    if case in ('purchased_cancel', QUALITY_GUIDE):
        assert last['dom']['focusedRow'] == '0' and proof == proof[:1] and not wire['calls']
        assert not records and not list((root / 'run/download-output').iterdir())
        if case == QUALITY_GUIDE:
            opened = stages['guide_opened']
            assert opened['dom']['qualityModal'] and opened['dom']['quality'] == expected_quality
            assert purchased(opened) == proof and not opened['voiceSongDownloadTrace']['calls']
            call = action(opened, 'openVoiceSongPurchasedQualityGuide')
            assert call['args'] == [target] and call['completed'] is True and call.get('result') is None
            assert action(last, 'openVoiceSongPurchasedQualityGuide') == call
            assert (root / 'browser-arguments').read_text(encoding='utf-8').splitlines() == ['1', STANDARD_GUIDE]
        return {'syntheticCalls': 0, 'playlistWrites': 0, 'mediaBytes': 0,
                **({'fixedBrowserHandoffVerified': True} if case == QUALITY_GUIDE else {})}
    if case in (NO_DIRECTORY, DIRECTORY_SETTINGS):
        prompt = stages['directory']
        assert prompt['dom']['directory'] and prompt['dom']['directoryTitle'] == '更改下载目录', prompt['dom']
        assert prompt['dom']['directoryButtons'] == ['取消下载', '更改目录'] and prompt['dom']['notice'] is None
        assert [call['args'][1]['kind'] for call in proof] == ['prepare', 'select', 'start', 'cancel'], proof
        assert purchased(prompt) == proof[:3]
        assert proof[1]['args'] == [target, {'kind': 'select', 'quality': expected_quality}]
        selected = proof[1]['result']
        assert selected['kind'] == 'selection' and selected['decision'] == 'ready' and selected['quality'] == expected_quality
        selection = selected['selection']
        assert selection and proof[2]['args'] == [target, {'kind': 'start', 'selection': selection,
                                                           'continuation': None,
                                                           'options': {**PREFS, 'quality': expected_quality}}]
        assert proof[2]['result'] == {'kind': 'directory_required'}
        assert proof[3]['args'] == [target, {'kind': 'cancel', 'selection': selection}]
        assert proof[3]['result'] == {'kind': 'cancelled'}
        if case == NO_DIRECTORY:
            assert last['dom']['focusedRow'] == '0'
        else:
            settings = stages['settings']['dom']
            assert settings['settingsFocus'] and settings['directoryInput'] == '', settings
            assert settings['chooserButton'] is True and settings['saveDisabled'] is True, settings
            assert last['dom']['settings'] is False and last['dom']['downloads'] is True, last['dom']
            assert_saved_directory_settings(root)
        assert not any(call['name'] == 'playlistCollectPage' for call in last['commands'])
        assert not wire['calls'] and not records and not list((root / 'run/download-output').iterdir())
        assert all(not stage['voiceSongDownloadTrace']['calls'] and stage['records']['directory'] is None
                   for stage in stages.values())
        return {'syntheticCalls': 0, 'playlistWrites': 0, 'mediaBytes': 0, 'directoryPromptVerified': True,
                **({'directorySettingsVerified': True} if case == DIRECTORY_SETTINGS else {})}
    choose = case == 'purchased_existing'
    assert [call['args'][1]['kind'] for call in proof] == (
        ['prepare', 'select', 'choose_playlist', 'start'] if choose else ['prepare', 'select', 'start']
    ), proof
    assert proof[1]['args'] == [target, {'kind': 'select', 'quality': expected_quality}]
    selected = proof[1]['result']
    assert selected['kind'] == 'selection' and selected['quality'] == expected_quality and selected['selection']
    assert selected['decision'] == ('playlist_required' if choose else 'ready')
    expected = []
    journal_ids = set()
    continuation = None
    if choose:
        select_stage = stages['picker']
        assert select_stage['dom']['pickerRows'] == ['创建新歌单', '下载到：合成已有下载歌单']
        assert purchased(select_stage) == proof[:2]
        collect = action(select_stage, 'playlistCollectPage')
        assert collect['args'] == [epoch, 'default', 0]
        assert [row['id'] for row in collect['result']['value']['playlists']] == ['81']
        expected.append([COLLECT, {'uid': '2', 'offset': 0, 'limit': 10, 'sortType': 'default'}])
        assert select_stage['voiceSongDownloadTrace']['calls'] == expected
        assert proof[2]['args'] == [target, {'kind': 'choose_playlist', 'selection': selected['selection'], 'playlistId': '81'}]
        destination = proof[2]['result']['picker']
        assert destination['playlistId'] == '81' and destination['phase'] == 'confirmed' and not destination['addFailed']
        assert destination['epoch'] == epoch and destination['playId'] == play_id and destination['trackId'] == '1710'
        journal_ids.add(destination['operationId'])
        assert records['playlist-mutations', destination['operationId']]['phase'] == 'confirmed'
        continuation = destination['continuation']
        assert continuation and continuation != selected['selection'] and continuation != destination['operationId']
        detail = {'id': '81', 'n': 100000, 's': 0}
        expected.extend([[DETAIL, detail, False], [ADD, {'pid': '81', 'trackIds': '["1710"]', 'op': 'add'}],
                         [DETAIL, detail, True]])
    else:
        assert not any(call['name'] == 'playlistCollectPage' for call in last['commands'])
    assert proof[-1]['args'] == [target, {'kind': 'start', 'selection': selected['selection'],
                                        'continuation': continuation, 'options': {**PREFS, 'quality': expected_quality}}]
    expected.extend([
        [AUTH, {'id': '1710', 'level': expected_quality, 'immerseType': 'c51'}],
        [META, {'c': '[{"id":"1710","v":0}]'}],
        ['media_get', {'offset': 0, 'bytes': MP3.stat().st_size}],
    ])
    assert wire['calls'] == expected, wire
    assert all(stage['voiceSongDownloadTrace']['calls'] ==
               expected[:len(stage['voiceSongDownloadTrace']['calls'])] for stage in stages.values())
    assert not last['records']['active']
    [task] = last['records']['done']
    assert (task['trackId'], task['source'], task['status'], task['requestedQuality'], task['quality']) == (
        '1710', 'song', 'complete', expected_quality, 'standard',
    )
    assert task['encrypted'] is False and task['bytes'] == MP3.stat().st_size
    accepted = proof[-1]['result']['result']
    assert accepted['accepted'] == [task['id']] and accepted['failed'] == []
    artifact = Path(task['path']).resolve(strict=True)
    assert artifact.parent == (root / 'run/download-output').resolve(strict=True)
    assert artifact.suffix == '.mp3'
    payload = artifact.read_bytes()
    assert payload == MP3.read_bytes()
    assert {key for (kind, key) in records if kind == 'playlist-mutations'} == journal_ids
    assert {key for (kind, key) in records if kind == 'download_tasks_v1'} == {task['id']}
    saved = records['download_tasks_v1', task['id']]
    assert saved['song_id'] == '1710' and saved['origin']['kind'] == 'song'
    assert saved['options'] == {**PREFS, 'quality': expected_quality}
    assert saved['status'] == 'complete' and saved['authorization_count'] == 1
    return {'syntheticCalls': len(expected), 'playlistWrites': len(journal_ids),
            'requestedQuality': expected_quality, 'actualQuality': 'standard', 'mediaBytes': len(payload),
            'mediaSha256': sha256(payload).hexdigest()}


def run(args, root, case):
    root.mkdir()
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    fixture = 'purchased_liked' if case in (NO_DIRECTORY, DIRECTORY_SETTINGS) else (
        'purchased_cancel' if case == QUALITY_GUIDE else case)
    env.update(CUMULUS_PROOF_DIR=str(root / 'run'), CUMULUS_VOICE_SONG_DOWNLOAD_PROOF=fixture,
               CUMULUS_PODCAST_SCENE_HOME_PROOF='automatic')
    if sys.platform == 'linux':
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime) + os.pathsep + env.get('PATH', '')
    script = args.script or (Path(__file__).resolve().parents[2] /
                            'desktop/examples/voice_song_purchased_download_ui_probe.js')
    if case == QUALITY_GUIDE:
        # Real main-window command, safe stand-in for the operating system launcher.
        # Keep the isolated WebView and the fake launcher off the network.
        launcher = root / 'bin/xdg-open'
        launcher.parent.mkdir()
        launcher.write_text('#!/bin/sh\nprintf "%s\\n" "$#" "$1" >> "$CUMULUS_PROOF_BROWSER_ARGUMENTS"\n',
                            encoding='utf-8')
        launcher.chmod(0o700)
        env['PATH'] = str(launcher.parent) + os.pathsep + env.get('PATH', '')
        env['CUMULUS_PROOF_BROWSER_ARGUMENTS'] = str(root / 'browser-arguments')
        driver = root / 'quality-guide-driver.js'
        driver.write_text('window.__voiceSongPurchasedGuide = true;\n' +
                          script.read_text(encoding='utf-8'), encoding='utf-8')
        script = driver
    if case in (NO_DIRECTORY, DIRECTORY_SETTINGS):
        # Reuse the exact signed already-liked fixture without configuring a directory.
        action = 'settings' if case == DIRECTORY_SETTINGS else 'cancel'
        driver = root / 'no-directory-driver.js'
        driver.write_text(f'window.__voiceSongPurchasedDirectoryAction = {json.dumps(action)};\n' +
                          script.read_text(encoding='utf-8'),
                          encoding='utf-8')
        script = driver
    env['CUMULUS_UX_SCRIPT'] = str(script.resolve(strict=True))
    isolated_profile(env, root)
    output = root / 'run/first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, (root / 'host.log').read_text(encoding='utf-8')
    stages = {p.stem.split('_', 2)[2]: read(p) for p in sorted(output.glob('ux_*.json'))}
    result = verify(case, root, stages, read(output / 'complete.json'))
    if sys.platform == 'linux':
        locale = read(output / 'locale.json')
        assert locale['numeric'] == 'C' and all(locale[key] != 'C' for key in ('messages', 'characters')), locale
    pngs = list(output.glob('ux_*.png'))
    assert len(pngs) == (len(stages) if args.screenshots else 0), pngs
    return {'case': case, 'stages': len(stages), 'screenshots': len(pngs), **result}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path, help='Windows runtime containing libmpv-2.dll')
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--case', choices=('all', *LABELS), default='all',
                        help='run the normal set or explicitly select the directory settings case')
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--script', type=Path, help='override only the synthetic driver')
    args = parser.parse_args()
    if sys.platform not in ('linux', 'win32') or (args.screenshots and sys.platform != 'linux'):
        parser.error('use Linux X11 or Windows; screenshots require X11')
    if args.case == QUALITY_GUIDE and sys.platform != 'linux':
        parser.error('the browser handoff receipt uses the Linux xdg-open stand-in')
    if sys.platform == 'linux':
        require_xvfb()
    if args.runtime:
        args.runtime = args.runtime.resolve(strict=True)
        if not args.runtime.is_dir():
            parser.error('--runtime must be a directory')
    if sys.platform == 'win32' and (not args.runtime or not (args.runtime / 'libmpv-2.dll').is_file()):
        parser.error('Windows requires --runtime with the packaged libmpv-2.dll')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    cases = (case for case in LABELS if case not in (DIRECTORY_SETTINGS, QUALITY_GUIDE)) if args.case == 'all' else (args.case,)
    results = [run(args, root / case, case) for case in cases]
    report = {'actualWebView': True, 'actualNullAudio': True,
              'webViewRuntime': 'WebKitGTK' if sys.platform == 'linux' else 'WebView2',
              'cases': results, 'realMusicRequests': 0, 'realAccountWrites': 0, 'physicalAudioVerified': False}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
