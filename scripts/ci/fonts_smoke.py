"""Real native font enumeration and bounded font picker in the Tauri WebView.

The directory error and 12005-family stress case are explicit IPC fixtures;
ordinary selection, rendering, persistence and restart use native font data.
"""
import argparse
import json
import os
from pathlib import Path
from desktop_lyrics_driver import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_FONT_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    total = 0
    phases = []
    for phase in ['first', 'reopen']:
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'font probe exit {code}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['realMusicRequests'] == 0
        assert complete['factoryCount'] == 2 and complete['mainVisible']
        assert complete['nativeFamilies'] > 0
        native = json.loads((output / 'native-input.json').read_text(encoding='utf-8'))
        assert native['keys'] == ([] if phase == 'reopen' else ['Tab', 'Escape'])
        states = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
        assert len(states) == complete['stageCount'] == (1 if phase == 'reopen' else 9)
        assert len(complete['checks']) == (2 if phase == 'reopen' else 8)
        assert all(not s['overflow'] and s['rows'] <= 20 for s in states)
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == len(states)
        total += len(states)
        phases.append(complete)
    report = dict(ok=True, states=total, screenshots=total if args.screenshots else 0,
                  realMusicRequests=0, physicalDisplayVerified=False, phases=phases)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
