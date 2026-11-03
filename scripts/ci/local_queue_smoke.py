"""Verify the local-library add-all action with real WebView, files and null audio."""
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
    parser.add_argument('--small', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_LOCAL_QUEUE_PROOF='1')
    if args.small:
        env['CUMULUS_PROOF_SMALL'] = '1'
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        path = root / 'os-profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([str(args.binary.resolve())], env=env, stdout=log,
                                stderr=subprocess.STDOUT, timeout=90, check=False)
    failed = run / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result.returncode == 0, f'local queue UI exit {result.returncode}'
    started = json.loads((run / 'local_queue_started.json').read_text(encoding='utf-8'))
    assert started == {'stage': 'local_queue_started', 'queue': 1, 'cue': True}
    complete = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['retainedPlayback'] and complete['realMusicRequests'] == 0
    assert complete['folderTree'] is True and complete['groupMenu'] is True
    if args.small:
        assert complete['width'] == 760
    assert complete['audioOutput'] == 'null' and complete['queue'] == ['夜行 · 下篇', '雨声', '夜行 · 上篇']
    print(json.dumps({'ok': True, 'actualWebView': True, 'queueCount': len(complete['queue'])}))


if __name__ == '__main__':
    main()
