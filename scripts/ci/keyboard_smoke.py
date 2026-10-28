"""Actual WebView keyboard ownership with real synthetic audio/null playback."""
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
    parser.add_argument('--startup-delay-ms', type=int, default=750, help='bounded synthetic native initialization delay')
    args = parser.parse_args()
    assert 0 <= args.startup_delay_ms <= 5000
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYER_START_DELAY_MS=str(args.startup_delay_ms),
               CUMULUS_UX_SCRIPT=str(Path('desktop/examples/keyboard_ui_probe.js').resolve()))
    for key, name in [
        ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
        ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
        ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'),
    ]:
        folder = root / 'os-profile' / name
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    with (root / 'run.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failure = root / 'first/failed.json'
    if failure.exists():
        raise AssertionError(failure.read_text(encoding='utf-8'))
    assert status == 0, status
    report = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert report['ok'] and report['stageCount'] == 7 and report['realMusicRequests'] == 0
    assert report['initialPlayEnabled']
    assert report['audioFactoryDelayRequestedMs'] == args.startup_delay_ms
    assert report['audioFactoryElapsedMs'] >= args.startup_delay_ms
    assert report['nativePlayer']['state'] == 'paused' and report['nativePlayer']['position'] > 0
    snapshots = list((root / 'first').glob('ux_*.json'))
    assert len(snapshots) == 7
    values = [json.loads(path.read_text(encoding='utf-8')) for path in snapshots]
    assert len({v['nativePlayer']['playId'] for v in values}) == 1
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == 7
    result = {'ok': True, 'actualWebView': True, 'states': 7,
              'screenshots': 7 if args.screenshots else 0, 'samePlayId': True, 'startupDelayMs': args.startup_delay_ms,
              'realMusicRequests': 0, 'physicalAudioOutputVerified': False}
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
