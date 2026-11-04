"""Exercise external playlist authorization and its durable receipt with a closed desktop API."""
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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/external_import_ui_probe.js'
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_EXTERNAL_IMPORT_PROOF='1', CUMULUS_UX_SCRIPT=str(source))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    if args.screenshots:
        env['CUMULUS_PROOF_CAPTURE_ACK'] = '1'
    for variable in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME',
                     'XDG_RUNTIME_DIR', 'APPDATA', 'LOCALAPPDATA'):
        directory = root / 'os-profile' / variable
        directory.mkdir(parents=True, mode=0o700)
        env[variable] = str(directory)
    receipts = {}
    for mode in ('first', 'reopen'):
        if mode == 'reopen':
            fixture = root / 'reopen-probe.js'
            fixture.write_text('window.__externalImportReopen = true;\n' +
                               source.read_text(encoding='utf-8'), encoding='utf-8')
            env.update(CUMULUS_PROOF_REOPEN='1', CUMULUS_UX_SCRIPT=str(fixture))
        output = root / mode
        with (root / (mode + '.log')).open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0
        def record(name):
            return json.loads((output / f'{name}.json').read_text(encoding='utf-8'))
        result = record('complete')
        assert result['ok'] and result['reopened'] == (mode == 'reopen')
        assert result['realMusicRequests'] == result['remoteAccountWrites'] == 0
        if mode == 'first':
            assert result['externalImport'] == {'requests': [
                '/api/login/qrcode/unikey', *(['/api/login/qrcode/client/login'] * 3),
                '/api/w/nuser/account/get', 'submit', 'submit', 'submit',
            ], 'dispatches': 3}
            assert record('ux_preview')['externalImport']['requests'] == []
            assert record('ux_cancelled')['externalImport']['dispatches'] == 0
            assert record('ux_text_edited')['externalImport']['dispatches'] == 1
            assert record('ux_text_confirm')['externalImport']['dispatches'] == 1
            assert record('ux_text_accepted')['externalImport']['dispatches'] == 2
            assert record('ux_checked')['externalImport']['dispatches'] == 3
        else:
            assert result['externalImport'] == {'requests': [], 'dispatches': 0}
        stages = 11 if mode == 'first' else 1
        assert len(list(output.glob('ux_*.json'))) == stages
        assert len(list(output.glob('ux_*.png'))) == (stages if args.screenshots else 0)
        receipts[mode] = result['externalImport']
    report = {'ok': True, 'actualWebView': True, 'states': 12,
              'screenshots': 12 if args.screenshots else 0,
              'realMusicRequests': 0, 'remoteAccountWrites': 0, 'synthetic': receipts}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
