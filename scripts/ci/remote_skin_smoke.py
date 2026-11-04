"""Test the official remote skin reading and image preview in a closed UX WebView."""
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
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/remote_skin_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(source),
               CUMULUS_REMOTE_SKIN_PROOF='1')
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    for name in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'APPDATA', 'LOCALAPPDATA'):
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[name] = str(directory)
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    if (output / 'failed.json').exists():
        raise AssertionError((output / 'failed.json').read_text(encoding='utf-8'))
    assert status == 0, f'WebView exited {status}'
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['stageCount'] == 9
    expected = {
        'catalogue_loading': (1, 0, 0), 'catalogue_failed': (1, 0, 0),
        'catalogue_ready_top': (2, 1, 0), 'catalogue_read': (2, 1, 0), 'preview_loading': (2, 1, 1),
        'preview_failed': (2, 1, 1), 'preview_ready': (2, 1, 2),
        'preview_closed': (2, 1, 2), 'catalogue_empty': (3, 1, 2),
    }
    observed = {}
    for item in output.glob('ux_*.json'):
        report = json.loads(item.read_text(encoding='utf-8'))
        calls = report['remoteSkinFixture']
        observed[item.stem.split('_', 2)[2]] = tuple(calls[k] for k in ('lists', 'covers', 'previews'))
    assert observed == expected, observed
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(expected)
        assert len(list(output.glob('paint-ux_*.json'))) == 7
    report = {'ok': True, 'actualWebView': True, 'states': 9,
              'screenshots': 9 if args.screenshots else 0,
              'syntheticCalls': complete['remoteSkinFixture']}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
