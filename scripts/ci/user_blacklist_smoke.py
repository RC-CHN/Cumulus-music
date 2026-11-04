"""Real settings and profile WebView proof with a synthetic user blacklist."""
import argparse
import json
import os
from pathlib import Path

from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/user_blacklist_ui_probe.js'
    run = root / 'run'
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_UX_SCRIPT=str(script))
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    output = run / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'probe exited {status}'
    host = (root / 'host.log').read_text(encoding='utf-8')
    assert 'panicked at' not in host, host
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['realMusicRequests'] == complete['remoteWrites'] == 0
    assert not complete['targetBlocked']
    stages = {path.stem.split('_', 2)[2]: json.loads(path.read_text(encoding='utf-8'))
              for path in sorted(output.glob('ux_*.json'))}
    assert len(stages) == complete['stageCount'] == 12 and all(s['visible'] for s in stages.values())
    assert stages['confirm']['confirm'] and not stages['cancelled']['confirm']
    assert stages['pending']['pending'] and stages['pending']['target']
    assert not stages['reconciled']['pending'] and not stages['reconciled']['target']
    list_ = ['/api/blacklist/get', {'userid': '2'}]
    calls = complete['calls']
    writes = [(target, body) for target, body in calls if target in ('/api/blacklist/add', '/api/blacklist/delete')]
    assert writes == [(f'/api/blacklist/{op}', {'userid': '73'}) for op in ('delete', 'add', 'delete')], calls
    assert calls[:2] == [list_, list_] and calls[-3:] == [list(writes[-1]), list_, list_], calls
    pictures = len(list(output.glob('ux_*.png')))
    assert not args.screenshots or pictures == len(stages)
    result = {'actualWebView': True, 'screenshots': pictures, 'states': list(stages), **complete}
    (root / 'report.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n',
                                    encoding='utf-8')
    print(json.dumps(result, ensure_ascii=False))


if __name__ == '__main__':
    main()
