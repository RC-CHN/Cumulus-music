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
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, False)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'session renewal process exited {code}'
    captures = len(list(output.glob('renewal_*.png')))
    assert captures == 0
    (output/'capture.json').write_text(json.dumps(dict(ok=True, exit_code=code, screenshots=captures))+'\n', encoding='utf-8')
    audit = Path(__file__).resolve().parents[2]/'desktop/examples/audit_renewal_probe.py'
    subprocess.run([sys.executable, str(audit), str(root/'run'), '--no-screenshots'], check=True)


if __name__ == '__main__':
    main()
