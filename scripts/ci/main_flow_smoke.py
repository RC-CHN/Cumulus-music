"""Single production WebView flow with closed synthetic API/media backends."""
import argparse
import hashlib
import json
import os
import sqlite3
from contextlib import closing
from pathlib import Path

from window_manager import launch

STAGES = (
    'recovered', 'daily', 'search', 'playback', 'playlist_created', 'playlist_edited',
    'playlist_cleaned', 'download_paused', 'download_complete', 'local_offline_playback', 'download_disposed', 'complete',
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true', help='optional isolated Linux GTK screenshots')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root/'os-profile'/key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    stages = {name: json.loads((output / ('complete.json' if name == 'complete' else
                                        f'ux_{name}.json')).read_text(encoding='utf-8'))
              for name in STAGES}
    assert all(stage['authenticated'] for stage in stages.values())
    assert stages['recovered']['fixture']['calls']['/api/w/nuser/account/get'] >= 1
    assert stages['daily']['fixture']['calls']['/api/v3/discovery/recommend/songs'] >= 1
    assert stages['search']['fixture']['calls']['/api/search/song/list/page'] >= 1
    assert stages['playback']['nativeProgress'] and stages['playback']['paused']
    assert stages['playback']['fixture']['sourceLoads'] == 1
    assert stages['playback']['fixture']['calls']['/api/song/enhance/player/url/v1'] >= 1
    assert stages['playlist_created']['fixture']['disposablePresent']
    assert stages['playlist_edited']['fixture']['calls']['/api/batch'] == 1
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
    offline = stages['local_offline_playback']
    assert offline['nativeProgress'] and offline['fixture']['offline']
    assert offline['fixture']['sourceLoads'] == 1 and offline['fixture']['localLoads'] == 1
    assert offline['fixture']['mediaTransfers'] == downloaded['fixture']['mediaTransfers']
    assert offline['fixture']['calls']['/api/song/enhance/player/url/v1'] == downloaded['fixture']['calls']['/api/song/enhance/player/url/v1']
    disposal = stages['download_disposed']
    assert disposal['stoppedBeforeDelete']
    assert disposal['audit'] == {'id': proof['id'], 'fileAbsent': True, 'ledgerAbsent': True, 'sentinelUnchanged': True}
    for stage in (offline, disposal, stages['complete']):
        assert stage['fixture']['offline'] and not stage['fixture']['disposablePresent']
        assert stage['fixture']['offlineMediaAttempts'] == 0
        assert stage['fixture']['calls'] == downloaded['fixture']['calls']
        assert stage['fixture']['mediaTransfers'] == downloaded['fixture']['mediaTransfers']
        assert stage['fixture']['sourceLoads'] == 1 and stage['fixture']['localLoads'] == 1
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
    assert sorted(playlist) == [('2', kind, 'confirmed') for kind in ('create', 'delete', 'metadata')]
    assert not leftovers
    assert disposal_journal == [('2', proof['id'], 'delete', 'complete')]
    screenshots = len(list(output.glob('ux_*.png')))
    assert screenshots == (len(STAGES) - 1 if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, actualLibmpv=True, stages=len(STAGES), screenshots=screenshots, sqliteVerified=True,
                  realMusicRequests=0, realAccountWrites=0, syntheticOfflinePlayback=True, nativeDownloadDisposal=True, downloadIntegrityCheckpoint=True,
                  pausedPartialAndVerifiedRangeResume=True)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
