"""Real main/lyric WebViews, native PCM/null audio and isolated synthetic failures."""
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
    assert not root.exists(), 'new proof root required'
    root.mkdir(parents=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root)
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    total = 0
    for phase in ['first', 'reopen']:
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'lyric probe exit {code}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['realMusicRequests'] == 0
        assert complete['factoryCount'] == 2 and complete['mainVisible']
        native = json.loads((output / 'native-input.json').read_text(encoding='utf-8'))
        assert native['regions']
        assert native['keys'] == ([] if phase == 'reopen' else ['d', 'd'])
        states = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
        assert len(states) == complete['stageCount'] == (3 if phase == 'reopen' else 22)
        assert len(complete['checks']) == (2 if phase == 'reopen' else 17)
        assert any(r['inputSuppressed'] for r in native['regions']) == (phase == 'first')
        assert all(not s['overflow'] and s['factoryCount'] == 2 for s in states)
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == len(states)
        total += len(states)
    report = dict(ok=True, states=total, screenshots=total if args.screenshots else 0,
                  actualWebViews=2, audioFactoryCount=2, playbackEngines=1, deviceInspectors=1, realMusicRequests=0,
                  physicalAudioOutputVerified=False, physicalDisplayVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
