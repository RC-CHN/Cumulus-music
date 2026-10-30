"""Actual WebView speed menu and native playback controls, using synthetic media."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def verify(stages):
    assert len(stages) == 32
    for row in stages:
        assert row['documentContained'] and row['trigger']['tag'] == 'BUTTON' and row['trigger']['hit']
        player = row['nativePlayer']
        assert player['state'] == 'paused' and abs(player['position'] - 10) < .2 and not player['error']
        assert player['speed'] == row['speed']
        if row['popup']:
            assert row['paintSamples']
            assert abs(row['popup']['width'] - 178) < 1
            assert len(row['popup']['rows']) == 9
            selected = [item for item in row['popup']['rows'] if item['checked']]
            assert len(selected) == 1 and float(selected[0]['label'].rstrip('×')) == row['speed']
        else:
            assert row['trigger']['text'] == ('倍速' if row['speed'] == 1 else f"{row['speed']}×")
    return {'ok': True, 'actualWebView': True, 'states': len(stages),
            'realMusicRequests': 0, 'realAccountWrites': 0,
            'physicalAudioOutputVerified': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--baseline', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/playback_speed_ui_probe.js'
    script = root / 'speed.js'
    script.write_text('window.__speedBaseline=' + json.dumps(args.baseline) + ';\n'
                      + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'speed probe exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    count = 1 if args.baseline else 32
    assert complete['ok'] and complete['stageCount'] == count
    if not args.baseline:
        assert complete['speeds'] == [2, 1.5, 1.25, 1, .75, .5, .25, 3, 4]
        assert complete['resumedFromSeek']
        assert abs(complete['resumedPosition'] - 10 - complete['resumeElapsed'] * 1.25) < .35
        assert complete['realMusicRequests'] == complete['accountWrites'] == 0
        stages = [json.loads(p.read_text(encoding='utf-8')) for p in sorted((root / 'first').glob('ux_*.json'))]
        report = verify(stages)
    else:
        report = {'ok': True, 'baseline': True}
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == count
    report.update(screenshots=count if args.screenshots else 0, physicalDisplayVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
