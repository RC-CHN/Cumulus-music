"""Verify a partial trial rail with actual WebView, native playback, and a synthetic source."""
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
    parser.add_argument('--frontend-dist', type=Path, help='Local synthetic frontend comparison')
    parser.add_argument('--buffer', action='store_true', help='Inject an advisory cache timestamp only')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/trial_seek_ui_probe.js'
    if args.buffer:
        source = script.read_text(encoding='utf-8')
        script = root / 'trial-buffer.js'
        script.write_text('window.__trialBufferProof = true;\n' + source, encoding='utf-8')
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_TRIAL_SEEK_PROOF='1',
               CUMULUS_UX_SCRIPT=str(script))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        folder = root / 'os-profile' / key
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'trial seek probe exited {code}'
    inside, outside = [
        json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))
        for stage in ('ux_01_trial_inside', 'ux_02_trial_outside')
    ]
    assert inside['bounds'] == [20, 40] and inside['total'] == 100
    assert outside['blocked'] == [10, 70]
    assert all(stage['seekCount'] == 1 and stage['nativePlayer']['state'] == 'paused'
               and abs(stage['nativePlayer']['position'] - 30) < .2 for stage in (inside, outside))
    assert inside['nativePlayer']['playId'] == outside['nativePlayer']['playId']
    if args.buffer:
        buffer = json.loads((output / 'ux_03_trial_buffer.json').read_text(encoding='utf-8'))
        assert buffer['injectedAdvisoryEnd'] == 60 and buffer['seekCount'] == 1
        left, right = buffer['observedSpan']
        assert abs(left - .3) < .01 and abs(right - .4) < .01
        assert buffer['nativePlayer']['state'] == 'paused'
        assert buffer['nativePlayer']['playId'] == inside['nativePlayer']['playId']
        assert abs(buffer['nativePlayer']['position'] - 30) < .2
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == (3 if args.buffer else 2)
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['seekCount'] == 1
    print(json.dumps(dict(ok=True, actualWebView=True, nativeSeekCount=1, realMusicRequests=0,
                          syntheticBufferAdvisory=args.buffer)))


if __name__ == '__main__':
    main()
