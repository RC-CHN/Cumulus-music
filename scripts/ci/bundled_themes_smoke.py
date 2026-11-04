"""Check bundled skin navigation, real paint and persistence in the isolated UX WebView."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True, help='synthetic ux_ui_probe executable')
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    for name in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'APPDATA', 'LOCALAPPDATA'):
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[name] = str(directory)
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/bundled_themes_ui_probe.js'
    phases = {
        'first': {
            'light': 'light', 'dark': 'dark', 'red': 'red', 'live_slider_preview': 'custom',
            'cancelled_to_red': 'red', 'custom_applied': 'custom', 'compact_uncommitted_exit': 'custom',
        },
        'reopen': {'reopened_applied': 'custom', 'reopened_cancel': 'custom'},
    }
    for phase, expected in phases.items():
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        script = root / f'{phase}-probe.js'
        script.write_text(
            f'window.__CUMULUS_BUNDLED_THEME_REOPEN__ = {json.dumps(phase == "reopen")};\n'
            + source.read_text(encoding='utf-8'), encoding='utf-8',
        )
        env['CUMULUS_UX_SCRIPT'] = str(script)
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
        if (output / 'failed.json').exists():
            raise AssertionError((output / 'failed.json').read_text(encoding='utf-8'))
        assert status == 0, f'{phase} WebView exited {status}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['reopened'] == (phase == 'reopen')
        assert complete['stageCount'] == len(expected)
        stages = {
            item.stem.split('_', 2)[2]: json.loads(item.read_text(encoding='utf-8'))['theme']
            for item in output.glob('ux_*.json')
        }
        assert stages == expected, stages
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == len(expected)
            assert len(list(output.glob('paint-ux_*.json'))) == len(expected)
    report = {'ok': True, 'actualWebView': True, 'states': 9,
              'screenshots': 9 if args.screenshots else 0, 'processRestart': True}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
