"""Real WebView effect catalogue using production IPC and a synthetic API."""
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
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/effect_directory_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_EFFECT_DIRECTORY_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['stageCount'] == 7 and complete['artworkVerified'] and complete['devicesVerified']
    catalogue = '/api/usertool/sound/queryAllSound'
    detail = '/api/usertool/home/sound/info'
    rights = '/api/vipauth/app/interests/auth/query'
    brands = '/api/usertool/home/soundeffect/devicebrand/list'
    models = '/api/usertool/home/soundeffect/devicesound/list'
    calls = complete['effectDirectoryCalls']
    assert [call['path'] for call in calls] == [
        catalogue, detail, rights, catalogue, detail, rights, detail, rights, catalogue,
        brands, models, models,
    ]
    assert [call['args']['exposeType'] for call in calls if call['path'] == catalogue] == [5, 6, 7]
    assert [call['args'] for call in calls if call['path'] in (brands, models)] == [
        {}, {'deviceManagerId': 4100}, {'deviceManagerId': 4200},
    ]
    assert len(list(output.glob('ux_*.png'))) == (7 if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'syntheticReadCalls': len(calls),
                      'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
