"""Production quality menu, native authorization, durable preference and restart."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def verify(stages, reopened=False):
    assert len(stages) == (3 if reopened else 35)
    for row in stages:
        assert row['documentContained'] and row['trigger']['tag'] == 'BUTTON' and row['trigger']['hit']
        assert row['trigger']['text'] == '标准' and row['trigger']['description'] == '当前标准'
        player = row['nativePlayer']
        assert player['state'] == 'paused' and abs(player['position'] - 10) < .2 and not player['error']
        assert player['track']['id'] == row['trackId'] == row['expectedTrack']
        assert player['actualQuality'] == row['actual'] == 'standard'
        assert player['requestedQuality'] == row['requested']
        assert player['qualityPreferences']['quality'] == row['preferred']
        assert player['qualityPreferences']['damaged'] == row['damaged']
        if row['popup']:
            assert row['paintSamples'] and abs(row['popup']['width'] - 378) < 1
            focused = [r for r in row['popup']['rows'] if r['focused']]
            assert len(focused) == 1 and focused[0]['hit']
            assert len(row['popup']['rows']) == (1 if row['damaged'] else 11)
            if not row['damaged']:
                selected=[r for r in row['popup']['rows'] if r['checked']]
                assert len(selected) == 1 and selected[0]['label'].startswith('标准')
    return {'ok': True, 'actualWebView': True, 'states': len(stages), 'reopened': reopened,
            'realMusicRequests': 0, 'realAccountWrites': 0, 'physicalAudioOutputVerified': False}


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
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1', CUMULUS_QUALITY_UI_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/playback_quality_ui_probe.js'
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    reports = []
    for reopened, name in [(False, 'first'), (True, 'reopen')]:
        script = root / f'{name}.js'
        script.write_text('window.__qualityReopen=' + json.dumps(reopened) + ';\n'
                          + source.read_text(encoding='utf-8'), encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(script)
        if reopened:
            env['CUMULUS_PROOF_REOPEN'] = '1'
        with (root / f'{name}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, root / name, args.screenshots)
        failed = root / name / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'quality probe exited {code}'
        complete = json.loads((root / name / 'complete.json').read_text(encoding='utf-8'))
        count = 3 if reopened else 35
        assert complete['ok'] and complete['stageCount'] == count
        assert complete['realMusicRequests'] == complete['accountWrites'] == 0
        assert complete['nativeQualityReadVerified']
        assert complete['syntheticQuality']['metadataReads'] >= 7 and complete['syntheticQuality']['trialWrites'] == 0
        stages = [json.loads(p.read_text(encoding='utf-8')) for p in sorted((root / name).glob('ux_*.json'))]
        report = verify(stages, reopened)
        if args.screenshots:
            assert len(list((root / name).glob('ux_*.png'))) == count
        report.update(screenshots=count if args.screenshots else 0, physicalDisplayVerified=False)
        reports.append(report)
    (root / 'audit.json').write_text(json.dumps(reports, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(reports))


if __name__ == '__main__':
    main()
