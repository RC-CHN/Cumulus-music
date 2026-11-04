"""Actual WebView and native stop recovery with a closed libmpv/null audio fixture."""
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
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/playback_native_stop_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_PLAYBACK_NATIVE_STOP_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'

    def read(name):
        return json.loads((output / f'{name}.json').read_text(encoding='utf-8'))

    complete = read('complete')
    assert complete['ok'] and complete['nativeStopVerified']
    before, stopped, resumed, restarted = [read(stage) for stage in (
        'ux_01_playing', 'ux_02_stopped', 'ux_03_resumed', 'ux_04_restarted')]
    assert [row['player']['action'] for row in (before, stopped, resumed, restarted)] == ['暂停', '播放', '暂停', '暂停']
    assert len({row['player']['entryId'] for row in (before, stopped, resumed, restarted)}) == 1
    assert complete['idle']['loads'] == 1 and complete['final']['loads'] == 3
    assert complete['idle']['songAuthorizations'] == complete['final']['songAuthorizations'] == 1
    assert complete['idle']['observedNativeStops'] == 1
    assert len(list(output.glob('ux_*.png'))) == (4 if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'nativeStopVerified': True,
                      'nativeLoads': 3, 'syntheticSongAuthorizationReads': 1,
                      'realAccountWrites': 0, 'physicalAudioOutputVerified': False}))


if __name__ == '__main__':
    main()
