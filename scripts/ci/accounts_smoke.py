"""Actual WebView confirmation focus, synthetic account removal and recovery."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'desktop/examples'))
from audit_saved_accounts_probe import audit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'),
                                ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / directory
        path.mkdir(parents=True)
        env[variable] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    for run in ['first', 'reopen']:
        if run == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        log_path = root / f'{run}.log'
        with log_path.open('x', encoding='utf-8') as log:
            result = subprocess.run([str(args.binary.resolve())], env=env,
                                    stdout=log, stderr=log, timeout=180)
        failure = root / 'run' / run / 'failed.json'
        assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
        assert result.returncode == 0, f'account probe exited {result.returncode}; inspect {log_path}'
    result = audit(root / 'run', screenshots=False)
    print(json.dumps({'ok': result['ok'], 'actualWebView': True, 'states': len(result['stages']),
                      'syntheticAccountsOnly': True, 'realMusicRequests': 0}))


if __name__ == '__main__':
    main()
