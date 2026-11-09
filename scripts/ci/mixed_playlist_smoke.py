"""Real local library and mixed playlist WebView using a closed account fixture."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        path = root / 'profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_MIXED_PLAYLIST_PROOF='1')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, run, args.screenshots)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8')
    assert code == 0, f'mixed playlist WebView exited {code}'
    paths = sorted(run.glob('ux_*.json'))
    assert [p.stem for p in paths] == [
        'ux_01_add_dialog', 'ux_02_add_cancelled', 'ux_03_mixed_read', 'ux_04_local_playing',
        'ux_05_remove_dialog', 'ux_06_remove_cancelled', 'ux_07_removed',
    ]
    states = [json.loads(p.read_text(encoding='utf-8')) for p in paths]
    before_add, first, added, playing, before_remove, cancelled, removed = states
    done = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
    local = done['localId']
    assert done['ok'] and done['unchangedQueue'] and local == before_add['localId'] == first['localId']
    assert [row['nativeReceipt']['generation'] for row in
            (*states, done)] == [None, None, 1, 1, 1, 1, 2, 2]
    assert [row['nativeReceipt']['localMembers'] for row in
            (*states, done)] == [0, 0, 1, 1, 1, 1, 0, 0]
    assert all(row['nativeReceipt']['localId'] == local and row['nativeReceipt']['localPosition'] == 0
               for row in (added, playing, before_remove, cancelled))
    assert added['playlist'] == before_remove['playlist'] == cancelled['playlist'] == [local, *removed['playlist']]
    assert removed['playlist'] == ['701', 'a10c0000000000000000000000000001', '702']
    assert playing['queue'] == [local, '701', '702'] and playing['nativePosition'] > .08
    assert all(playing[key] == removed[key] for key in ('playId', 'nativePosition', 'queueRevision'))
    assert all(row['nativeReceipt']['originalUnchanged'] and
               not row['nativeReceipt']['remoteMutations'] and not row['nativeReceipt']['onlineAudio']
               for row in (*states, done))
    assert done['nativeReceipt']['detailReads'] >= 4
    assert ['701', '702'] in done['nativeReceipt']['catalogReads']
    assert all(set(ids) <= {'701', '702'} for ids in done['nativeReceipt']['catalogReads'])
    if os.name != 'nt':
        locale = json.loads((run / 'native-startup.json').read_text(encoding='utf-8'))
        assert locale['numericLocale'] == 'C' and locale['messageLocale'] not in ('C', 'POSIX')
    images = sorted(run.glob('ux_*.png'))
    assert len(images) == (len(paths) if args.screenshots else 0)
    result = dict(ok=True, actualWebView=True, nativeLocalPlayback=True, states=len(paths),
                  screenshots=len(images), originalFilePreserved=True, localGeneration=2,
                  remotePlaylistMutations=0, onlineAudioRequests=0, realMusicRequests=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
