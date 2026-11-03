"""Single production WebView flow with closed synthetic API/media backends."""
import argparse
import json
import os
import sqlite3
from contextlib import closing
from pathlib import Path

from window_manager import launch

STAGES = (
    'recovered', 'daily', 'search', 'playback', 'playlist_created', 'playlist_edited',
    'playlist_cleaned', 'download_complete', 'local_offline_playback', 'complete',
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
    downloaded = stages['download_complete']
    assert downloaded['bytes'] > 0 and downloaded['fixture']['mediaTransfers'] == 1
    files = list((root/'run/media').rglob('*.flac'))
    assert len(files) == 1 and files[0].stat().st_size == downloaded['bytes']
    seed_size = (root/'run/synthetic.flac').stat().st_size
    offline = stages['local_offline_playback']
    assert offline['nativeProgress'] and offline['fixture']['offline']
    assert offline['fixture']['sourceLoads'] == 1 and offline['fixture']['localLoads'] == 1
    assert offline['fixture']['mediaTransfers'] == downloaded['fixture']['mediaTransfers']
    assert offline['fixture']['calls']['/api/song/enhance/player/url/v1'] == downloaded['fixture']['calls']['/api/song/enhance/player/url/v1']
    assert stages['complete']['ok'] and stages['complete']['allInOneWebView']
    database = root/'run/state/core.sqlite3'
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True)) as state:
        playlist = state.execute(
            "SELECT owner,json_extract(value,'$.command.kind'),json_extract(value,'$.phase') "
            "FROM state WHERE kind='playlist-mutations'").fetchall()
        download = state.execute(
            "SELECT owner,json_extract(value,'$.status'),json_extract(value,'$.raw_integrity.bytes'),"
            "json_extract(value,'$.output_integrity.bytes'),json_extract(value,'$.cleanup_pending'),"
            "json_extract(value,'$.target') FROM state WHERE kind='download_tasks_v1'").fetchall()
        assert state.execute("SELECT 1 FROM state WHERE owner='device' AND kind='identity'").fetchone() is None
    assert sorted(playlist) == [('2', kind, 'confirmed') for kind in ('create', 'delete', 'metadata')]
    assert len(download) == 1
    assert download[0][:5] == ('2', 'complete', seed_size, downloaded['bytes'], 0)
    assert Path(download[0][5]).resolve() == files[0].resolve()
    screenshots = len(list(output.glob('ux_*.png')))
    assert screenshots == (len(STAGES) - 1 if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, actualLibmpv=True, stages=len(STAGES), screenshots=screenshots, sqliteVerified=True,
                  realMusicRequests=0, realAccountWrites=0, syntheticOfflinePlayback=True)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
