"""Real production WebView/IPC/pump with synthetic credentials and a native test clock."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--foreground', action='store_true',
                        help='exercise a real Linux or Windows foreground transition')
    args = parser.parse_args()
    if args.foreground and not (sys.platform.startswith('linux') or sys.platform == 'win32'):
        parser.error('--foreground requires a Linux or Windows desktop')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    if args.foreground:
        env['CUMULUS_PROOF_RENEWAL_FOREGROUND'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots,
                      capture_glob='renewal_*.json')
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'session renewal process exited {code}'
    captures = len(list(output.glob('renewal_*.png')))
    (output/'capture.json').write_text(json.dumps(dict(ok=True, exit_code=code, screenshots=captures))+'\n', encoding='utf-8')
    audit = Path(__file__).resolve().parents[2]/'desktop/examples/audit_renewal_probe.py'
    command = [sys.executable, str(audit), str(root/'run')]
    if not args.screenshots:
        command.append('--no-screenshots')
    if args.foreground:
        command.append('--foreground')
    subprocess.run(command, check=True)


if __name__ == '__main__':
    main()
