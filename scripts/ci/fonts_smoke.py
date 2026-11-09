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
    parser.add_argument('--force-ipc-fallback', action='store_true',
                        help='Fail one synthetic account read at the WebView fetch boundary')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_FONT_PROOF='1')
    if args.force_ipc_fallback:
        env['CUMULUS_FONT_PROOF_FORCE_IPC_FALLBACK'] = '1'
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
        assert complete['forcedFallback'] == (args.force_ipc_fallback and phase == 'first')
        native = json.loads((output / 'native-input.json').read_text(encoding='utf-8'))
        assert native['keys'] == ([] if phase == 'reopen' else ['Tab', 'Escape', 'Return'])
        states = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
        assert len(states) == complete['stageCount'] == (1 if phase == 'reopen' else 9)
        assert len(complete['checks']) == (2 if phase == 'reopen' else 8)
        assert all(not s['overflow'] and s['rows'] <= 20 for s in states)
        if phase == 'first':
            failed, retried = states[5:7]
            assert failed['stage'] == 'ux_06_read_failed' and failed['rows'] == 0
            assert retried['stage'] == 'ux_07_retry_native' and retried['rows'] > 0
            assert retried['fontReads'] == failed['fontReads'] + 1
            retry = retried['retry']
            assert retry['trusted'] and retry['clicks'] == retry['reads'] == 1, retry
            assert failed['fontSources']['failure'] == retried['fontSources']['failure'] == 1
            assert retried['fontSources']['native'] == failed['fontSources']['native'] + 1
            large, alias = states[7:9]
            assert large['stage'] == 'ux_08_large_directory' and large['rows'] == 20
            assert alias['stage'] == 'ux_09_alias_search' and alias['rows'] == 1
            assert large['fontSources']['large'] == alias['fontSources']['large'] == 1
            assert large['fontReads'] == alias['fontReads'] == retried['fontReads'] + 1
            assert large['catalog']['page'] == '2 / 601' and large['catalog']['count'] == '12005 种字体'
            assert [int(name.split()[2]) for name in large['catalog']['names']] == list(range(20, 40))
            assert alias['catalog']['page'] == '1 / 1' and alias['catalog']['count'] == '1 种字体'
            assert [int(name.split()[2]) for name in alias['catalog']['names']] == [12004]
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
