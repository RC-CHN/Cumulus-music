"""Verify local CUE playback in the actual WebView during a held synthetic restore."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_LOCAL_COLD_START_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([str(args.binary.resolve())], env=env, stdout=log,
                                stderr=subprocess.STDOUT, timeout=80, check=False)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result.returncode == 0, f'local cold-start UI process exit {result.returncode}'
    playing = json.loads((run / 'local_cold_playing.json').read_text(encoding='utf-8'))
    assert playing == {'stage': 'local_cold_playing', 'pending': True, 'advancing': True}
    complete = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
    assert complete == {'stage': 'complete', 'ok': True, 'localWhilePending': True,
                        'keptLocalPage': True, 'keptPlayback': True, 'realMusicRequests': 0}
    print(json.dumps(complete))


if __name__ == '__main__':
    main()
