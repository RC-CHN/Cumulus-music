"""Single production WebView flow with closed synthetic API/media backends."""
import argparse
import hashlib
import json
import os
import socket
import sqlite3
from contextlib import closing
from pathlib import Path

from window_manager import launch

STAGES = (
    'guest', 'qr_pending', 'login_adopted', 'recovered', 'daily', 'search', 'playback', 'playlist_created', 'playlist_edited',
    'playlist_song_added', 'playlist_song_removed', 'playlist_cleaned', 'download_paused', 'download_complete', 'local_offline_playback', 'download_disposed', 'complete',
)


def cut_after_publish(control, stage, proof, native_pid):
    if stage.name != 'ux_download_complete.json':
        return
    audit = proof['audit']
    assert proof['captureHandshake'] and proof['stage'] == 'ux_download_complete'
    assert not proof['fixture']['offline'] and proof['fixture']['mediaTransfers'] == 2
    assert audit['file'] == audit['taskIntegrity'] == audit['ledgerIntegrity']
    database = stage.parent.parent/'state/core.sqlite3'
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True, timeout=1)) as state:
        tasks = state.execute(
            "SELECT key, json_extract(value, '$.status'), json_extract(value, '$.options.quality'), "
            "json_extract(value, '$.resource.level'), json_extract(value, '$.song.fee'), "
            "json_extract(value, '$.song_id') = CAST(json_extract(value, '$.resource.id') AS TEXT) "
            "FROM state WHERE kind = 'download_tasks_v1'"
        ).fetchall()
    assert tasks == [(audit['id'], 'complete', 'standard', 'lossless', 0, 1)], 'unexpected synthetic download receipt'
    if Path(f'/proc/{native_pid}/ns/net').readlink() != Path('/proc/self/ns/net').readlink():
        raise RuntimeError('native application and capture driver do not share the isolated network')
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.settimeout(7)
        connection.connect(str(control))
        connection.sendall(b'C')
        if connection.recv(1) != b'Y':
            raise RuntimeError('network controller did not confirm cutoff')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--ordinary-https', action='store_true', help=argparse.SUPPRESS)
    special = parser.add_mutually_exclusive_group()
    special.add_argument('--https-retry-once', action='store_true', help=argparse.SUPPRESS)
    special.add_argument('--https-eof-next', action='store_true', help=argparse.SUPPRESS)
    special.add_argument('--loading-step', action='store_true')
    special.add_argument('--missing-download', action='store_true')
    parser.add_argument('--synthetic-offline-route', action='store_true',
                        help='simulate the native route observation without changing runner routes')
    parser.add_argument('--https-retry-with-play', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--screenshots', action='store_true', help='optional isolated Linux GTK screenshots')
    parser.add_argument('--loopback-cut-control', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if (args.https_retry_once or args.https_eof_next) and not args.ordinary_https:
        parser.error('HTTPS playback scenarios require --ordinary-https')
    if args.https_retry_with_play and not args.https_retry_once:
        parser.error('--https-retry-with-play requires --https-retry-once')
    if (args.loading_step or args.missing_download) and args.ordinary_https:
        parser.error('--loading-step and --missing-download use the synthetic decoder gate')
    if args.synthetic_offline_route and not args.missing_download:
        parser.error('--synthetic-offline-route requires --missing-download')
    if args.loopback_cut_control and (not args.ordinary_https or not args.screenshots
                                      or os.name != 'posix' or not args.loopback_cut_control.is_socket()):
        parser.error('the synthetic cutoff requires the isolated HTTPS controller and capture barrier')
    stage_names = STAGES
    expected_source_loads = 2 if args.https_eof_next else 1
    if args.https_retry_once or args.https_eof_next:
        position = STAGES.index('playback')
        checkpoint = 'https_failure' if args.https_retry_once else 'https_before_eof'
        stage_names = STAGES[:position] + (checkpoint,) + STAGES[position:]
    elif not args.ordinary_https and not args.loading_step:
        position = STAGES.index('playback')
        stage_names = STAGES[:position] + ('search_loading_cover', 'search_loading_rows') + STAGES[position:]
    if args.missing_download:
        position = stage_names.index('local_offline_playback')
        stage_names = stage_names[:position] + ('download_missing', 'download_online') + stage_names[position:]
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    if args.ordinary_https:
        env['CUMULUS_MAIN_FLOW_HTTPS_PROOF'] = '1'
    if args.https_retry_once:
        env['CUMULUS_MAIN_FLOW_HTTPS_RETRY'] = '1'
    if args.https_retry_with_play:
        env['CUMULUS_MAIN_FLOW_HTTPS_RETRY_WITH_PLAY'] = '1'
    if args.https_eof_next:
        env['CUMULUS_MAIN_FLOW_HTTPS_EOF'] = '1'
    if args.loading_step:
        env['CUMULUS_MAIN_FLOW_LOADING_STEP'] = '1'
    if args.missing_download:
        env['CUMULUS_MAIN_FLOW_MISSING_DOWNLOAD'] = '1'
    if args.synthetic_offline_route:
        env['CUMULUS_MAIN_FLOW_SYNTHETIC_ROUTE'] = '1'
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root/'os-profile'/key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots,
                      before_ack=(lambda *stage: cut_after_publish(args.loopback_cut_control, *stage))
                      if args.loopback_cut_control else None)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    stages = {name: json.loads((output / ('complete.json' if name == 'complete' else
                                        f'ux_{name}.json')).read_text(encoding='utf-8'))
              for name in stage_names}
    guest, pending, adopted = (stages[name] for name in ('guest', 'qr_pending', 'login_adopted'))
    assert not guest['authenticated'] and not pending['authenticated']
    assert guest['status'] == pending['status'] and guest['status'] in ('unknown', 'signed_out')
    assert guest['epoch'] == guest['nativeEpoch'] == pending['epoch'] == pending['nativeEpoch'] == adopted['previousEpoch']
    assert guest['wire']['tickets'] == 0 and pending['wire']['guestIdentityReads'] >= 1
    assert pending['wire']['tickets'] == 1 and pending['wire']['polls'] in (1, 2)
    assert not pending['wire']['approved'] and not pending['wire']['authorized']
    assert adopted['status'] == 'authenticated' and adopted['epoch'] != guest['epoch']
    assert adopted['audit'] == {'savedCurrentProfile': True}
    assert adopted['wire']['tickets'] == 1 and adopted['wire']['polls'] == 2
    assert adopted['wire']['approved'] and adopted['wire']['authorized']
    assert adopted['wire']['authenticatedIdentityReads'] >= 1
    assert all(stages[name]['authenticated'] and stages[name]['nativeEpoch'] == adopted['epoch']
               for name in stage_names[2:])
    assert stages['complete']['wire']['tickets'] == 1 and stages['complete']['wire']['polls'] == 2
    assert stages['recovered']['fixture']['calls']['/api/w/nuser/account/get'] >= 1
    assert stages['daily']['fixture']['calls']['/api/v3/discovery/recommend/songs'] >= 1
    assert stages['search']['fixture']['calls']['/api/search/song/list/page'] >= 1
    assert stages['playback']['nativeProgress'] and stages['playback']['paused']
    assert stages['playback']['fixture']['sourceLoads'] == expected_source_loads
    assert stages['playback']['fixture']['ordinaryHttps'] == args.ordinary_https
    assert stages['playback']['resumedSameEntry'] == args.ordinary_https
    assert stages['playback']['loadingSeekVerified'] == (not args.ordinary_https and not args.loading_step)
    assert stages['playback']['loadingPauseVerified'] == (not args.ordinary_https and not args.loading_step)
    assert stages['playback']['loadingStepVerified'] == args.loading_step
    assert stages['playback']['loadingSearchCoverVerified'] == (not args.ordinary_https and not args.loading_step)
    assert stages['playback']['loadingSearchRowsVerified'] == stages['playback']['loadingSearchCoverVerified']
    if stages['playback']['loadingSearchCoverVerified']:
        for stage in ('search_loading_cover', 'search_loading_rows'):
            held = stages[stage]
            assert held['samePendingSelection'] and held['fixture']['sourceLoads'] == 0
            assert held['fixture']['sourceAuthorizations'] == held['fixture']['sourceLoadAttempts'] == 1
        assert stages['playback']['fixture']['sourceAuthorizations'] == 1
        assert stages['playback']['fixture']['sourceLoadAttempts'] == 1
    if args.loading_step:
        assert stages['playback']['fixture']['sourceLoadAttempts'] == 2
        assert stages['playback']['fixture']['sourceAuthorizations'] == 2
    if args.https_retry_once:
        failed = stages['https_failure']
        assert failed['failedBeforeExplicitRetry'] and not failed['rawLocatorVisible']
        assert failed['fixture']['retryHttps']
        assert failed['fixture']['sourceAuthorizations'] == failed['fixture']['sourceLoadAttempts'] == 1
        assert failed['fixture']['sourceLoads'] == 0
        assert stages['playback']['fixture']['sourceAuthorizations'] == stages['playback']['fixture']['sourceLoadAttempts'] == 2
        assert stages['playback']['httpsRetryViaPlayVerified'] == args.https_retry_with_play
    if args.https_eof_next:
        before = stages['https_before_eof']
        assert before['firstTrackPlaying'] and before['fixture']['eofHttps']
        assert before['fixture']['sourceLoads'] == before['fixture']['sourceLoadAttempts'] == 1
        assert before['fixture']['sourceAuthorizations'] == 2
        assert stages['playback']['advancedByEof']
        assert stages['playback']['fixture']['sourceLoadAttempts'] == 2
        assert stages['playback']['fixture']['sourceAuthorizations'] == 2
    assert stages['playback']['fixture']['calls']['/api/song/enhance/player/url/v1'] >= 1
    assert stages['playlist_created']['fixture']['disposablePresent']
    assert stages['playlist_edited']['fixture']['calls']['/api/batch'] == 1
    added, removed = stages['playlist_song_added'], stages['playlist_song_removed']
    assert added['playlistId'] == removed['playlistId'] == '42'
    assert added['trackIds'] == ['1'] and removed['trackIds'] == []
    assert added['fixture']['calls']['/api/v1/playlist/manipulate/tracks'] == 1
    assert removed['fixture']['calls']['/api/v1/playlist/manipulate/tracks'] == 2
    cleaned = stages['playlist_cleaned']
    assert cleaned['fullBaselineRestored'] and not cleaned['fixture']['disposablePresent']
    assert cleaned['fixture']['calls']['/api/playlist/create'] == 1
    assert cleaned['fixture']['calls']['/api/playlist/delete'] == 1
    paused = stages['download_paused']
    assert paused['fixture']['mediaRequests'] == [[0, False]]
    assert paused['bytes'] > 0 and paused['audit'] == {
        'id': paused['taskId'], 'prefixBytes': paused['bytes'], 'prefixVerified': True,
    }
    downloaded = stages['download_complete']
    assert downloaded['bytes'] > paused['bytes'] and downloaded['fixture']['mediaTransfers'] == 2
    assert downloaded['fixture']['mediaRequests'] == [[0, False], [paused['bytes'], False]]
    assert downloaded['audit']['id'] == paused['taskId']
    assert downloaded['fixture']['calls']['/api/song/enhance/download/url/v1'] == 2
    proof = downloaded['audit']
    assert proof['file'] == proof['taskIntegrity'] == proof['ledgerIntegrity']
    assert proof['file']['bytes'] == downloaded['bytes'] and len(proof['file']['sha256']) == 64
    seed = (root/'run/synthetic.flac').read_bytes()
    assert proof['source'] == {'bytes': len(seed), 'sha256': hashlib.sha256(seed).hexdigest()}
    media = (root/'run/media').resolve()
    target = (media/proof['relative']).resolve()
    assert target.parent == media and target.suffix.lower() == '.flac'
    assert not os.path.lexists(media/proof['relative'])
    assert {path.relative_to(media).as_posix() for path in media.rglob('*') if path.is_file()} == {'unrelated.txt'}
    assert (media/'unrelated.txt').read_bytes() == b'unrelated synthetic file\n'
    authorization_stage = downloaded
    if args.missing_download:
        missing, online = (stages[name] for name in ('download_missing', 'download_online'))
        source = '/api/song/enhance/player/url/v1'
        assert missing['qualifiedMissing'] and online['sameSelectedEntry'] and online['nativeProgress']
        assert missing['fixture']['calls'][source] == downloaded['fixture']['calls'][source]
        assert missing['fixture']['sourceLoadAttempts'] == downloaded['fixture']['sourceLoadAttempts']
        assert missing['fixture']['localLoads'] == online['fixture']['localLoads'] == 0
        assert online['fixture']['calls'][source] == missing['fixture']['calls'][source] + 1
        assert online['fixture']['sourceLoads'] == expected_source_loads + 1
        assert online['fixture']['mediaTransfers'] == downloaded['fixture']['mediaTransfers']
        expected_source_loads += 1
        authorization_stage = online
    offline = stages['local_offline_playback']
    assert offline['nativeProgress'] and offline['fixture']['offline']
    assert offline['fixture']['sourceLoads'] == expected_source_loads and offline['fixture']['localLoads'] == 1
    assert offline['fixture']['mediaTransfers'] == downloaded['fixture']['mediaTransfers']
    assert offline['fixture']['calls']['/api/song/enhance/player/url/v1'] == authorization_stage['fixture']['calls']['/api/song/enhance/player/url/v1']
    disposal = stages['download_disposed']
    assert disposal['stoppedBeforeDelete']
    if args.synthetic_offline_route:
        assert offline['syntheticRouteReads'] == 0
        assert disposal['syntheticRouteReads'] == stages['complete']['syntheticRouteReads'] == 1
    assert disposal['audit'] == {'id': proof['id'], 'fileAbsent': True, 'ledgerAbsent': True, 'sentinelUnchanged': True}
    for stage in (offline, disposal, stages['complete']):
        assert stage['fixture']['offline'] and not stage['fixture']['disposablePresent']
        assert stage['fixture']['offlineMediaAttempts'] == 0
        assert stage['wire']['offlineBusinessAttempts'] == 0, (
            stage['stage'], stage['wire'].get('offlineBusinessPaths'),
            stage['wire'].get('offlineUnattributedAttempts'),
        )
        assert stage['fixture']['calls'] == authorization_stage['fixture']['calls']
        assert stage['fixture']['mediaTransfers'] == downloaded['fixture']['mediaTransfers']
        assert stage['fixture']['sourceLoads'] == expected_source_loads and stage['fixture']['localLoads'] == 1
    assert stages['complete']['ok'] and stages['complete']['allInOneWebView']
    database = root/'run/state/core.sqlite3'
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True)) as state:
        playlist = state.execute(
            "SELECT owner,json_extract(value,'$.command.kind'),json_extract(value,'$.phase') "
            "FROM state WHERE kind='playlist-mutations'").fetchall()
        leftovers = state.execute(
            "SELECT kind FROM state WHERE kind IN ('download_tasks_v1','offline_music_v1','media_transfers_v1')"
        ).fetchall()
        disposal_journal = state.execute(
            "SELECT owner,json_extract(value,'$.task_id'),json_extract(value,'$.action'),"
            "json_extract(value,'$.phase') FROM state WHERE kind='download_file_operations_v1'"
        ).fetchall()
        assert state.execute("SELECT 1 FROM state WHERE owner='device' AND kind='identity'").fetchone() is None
    assert sorted(row for row in playlist if row[1] == 'tracks') == [('2', 'tracks', 'confirmed')] * 2
    assert sorted(row for row in playlist if row[1] != 'tracks') == [('2', kind, 'confirmed') for kind in ('create', 'delete', 'metadata')]
    assert not leftovers
    assert disposal_journal == [('2', proof['id'], 'delete', 'complete')]
    # New process, same native state, entirely fresh OS/WebView storage.
    reopen_logs = root/'reopen-logs'
    reopen_logs.mkdir()
    reopen_env = {**env, 'CUMULUS_PROOF_REOPEN': '1'}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root/'reopen-os-profile'/key
        profile.mkdir(parents=True, mode=0o700)
        reopen_env[key] = str(profile)
    reopened_output = root/'run/reopen'
    with (reopen_logs/'host.log').open('x', encoding='utf-8') as log:
        reopen_code = launch(args.binary.resolve(), reopen_env, log, reopened_output, args.screenshots)
    failed = reopened_output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert reopen_code == 0
    reopened = json.loads((reopened_output/'ux_reopened.json').read_text(encoding='utf-8'))
    assert reopened['ok'] and reopened['authenticated'] and reopened['status'] == 'authenticated'
    assert reopened['nativeEpoch'] == reopened['epoch']
    assert reopened['baselineCount'] == 1 and reopened['baselineOwned']
    assert reopened['audit'] == {'savedCurrentProfile': True}
    assert reopened['wire']['mode'] == 'reopen' and reopened['wire']['authorized']
    assert reopened['wire']['tickets'] == reopened['wire']['polls'] == 0
    assert not reopened['wire']['approved'] and reopened['wire']['authenticatedIdentityReads'] >= 1
    assert reopened['fixture']['calls']['/api/user/playlist'] >= 1
    assert reopened['fixture']['mediaTransfers'] == reopened['fixture']['sourceLoads'] == 0
    screenshots = sum(len(list(folder.glob('ux_*.png'))) for folder in (output, reopened_output))
    assert screenshots == (len(stage_names) if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, actualLibmpv=True, stages=len(stage_names)+1, screenshots=screenshots, sqliteVerified=True,
                  ordinaryHttpsPlayback=args.ordinary_https, resumedSameHttpsEntry=args.ordinary_https,
                  explicitHttpsRetryVerified=args.https_retry_once,
                  realMusicRequests=0, realAccountWrites=0, syntheticOfflinePlayback=True, nativeDownloadDisposal=True, downloadIntegrityCheckpoint=True,
                  pausedPartialAndVerifiedRangeResume=True, playlistMembershipReadback=True,
                  syntheticQrAdoption=True, savedCurrentSyntheticProfile=True,
                  freshProcessRestoredSyntheticProfile=True, processLifetimes=2,
                  rejectedOfflineRelayExperimentAttempts=stages['complete']['wire']['offlineRelayExperimentAttempts'])
    if args.https_eof_next:
        report['nativeHttpsEofAdvanced'] = True
    if args.https_retry_with_play:
        report['mainPlayRecoveredFirstHttpsOpen'] = True
    if args.missing_download:
        report['missingDownloadPlayedOnlyAfterExplicitAction'] = True
    if args.synthetic_offline_route:
        report['syntheticOfflineRouteObservedByNativeMenu'] = True
    if args.loopback_cut_control:
        report['hardCutAfterStandardDownloadRequest'] = True
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
