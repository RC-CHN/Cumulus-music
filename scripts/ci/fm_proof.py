"""Launch a real FM WebView proof in a fresh, synthetic-only account profile."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def arguments(description):
    parser = argparse.ArgumentParser(description=description)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    return parser.parse_args()


def run(args, case):
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples'
    script = root / 'fm-proof.js'
    script.write_text((source/'support/fm_proof_tools.js').read_text(encoding='utf-8') + '\n' +
                      (source/case).read_text(encoding='utf-8'), encoding='utf-8')
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_FM_FEEDBACK_UI_PROOF='1', CUMULUS_UX_SCRIPT=str(script))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root/'first', args.screenshots)
    failure = root/'first/failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert code == 0, f'FM proof exited {code}'
    complete = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    return root, complete
