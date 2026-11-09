"""Actual offline playlist WebView with the core talking only to a sealed adapter."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))}
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / 'profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_OFFLINE_PLAYLIST_PROOF='1')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, run, args.screenshots)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8')
    assert code == 0, f'offline playlist proof exited {code}'
    stages = ['ux_01_live', 'ux_02_cached', 'ux_03_cue', 'ux_04_retry_offline',
              'ux_05_reconnected', 'ux_06_denied', 'ux_07_no_cache']
    files = sorted(run.glob('ux_*.json'))
    assert [file.stem for file in files] == stages
    states = [json.loads(file.read_text(encoding='utf-8')) for file in files]
    done = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
    assert done['ok']
    local = done['local']
    expected = [local, '701', 'a10c0000000000000000000000000001', '702']
    assert [state['rows'] for state in states] == [expected] * 5 + [[], []]
    receipts = [state['nativeReceipt'] for state in states]
    assert [r['cachedRows'] for r in receipts] == [4] * 5 + [0, 0]
    assert [r['unavailable'] for r in receipts] == [0, 1, 1, 2, 2, 3, 4]
    assert [r['denied'] for r in receipts] == [0] * 5 + [1, 1]
    assert all(r['cueUnchanged'] and r['cachedWithoutUrls'] and r['mixed']['originalUnchanged'] and
               not r['mixed']['catalogAudioChecks'] and not r['mixed']['unsafeAudioAttempts'] and
               not r['mixed']['remoteMutations'] for r in receipts)
    assert [r['mixed']['catalogReads'] for r in receipts[:4]].count(receipts[0]['mixed']['catalogReads']) == 4
    assert states[2]['queue'] == [local] and states[2]['position'] > .08
    assert 7900 <= states[2]['durationMs'] <= 8100 and states[2]['playId']
    locale = json.loads((run / 'native-startup.json').read_text(encoding='utf-8'))
    assert locale['numericLocale'] == 'C' and locale['messageLocale'] not in ('C', 'POSIX')
    images = sorted(run.glob('ux_*.png'))
    assert len(images) == (len(stages) if args.screenshots else 0)
    result = dict(ok=True, actualWebView=True, nativeCuePlayback=True, stages=len(stages),
                  screenshots=len(images), hardDenialInvalidatesCache=True, realMusicRequests=0,
                  playlistWrites=0, onlineAudioRequests=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
