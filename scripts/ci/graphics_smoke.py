"""Exercise the shipped graphics-only startup path, with no account initialization."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    reports = []
    modes = ['auto', 'auto', 'software']
    if os.name != 'nt':
        modes.append('gpu-shm')
    for index, mode in enumerate(modes):
        env = dict(base, CUMULUS_DATA_DIR=str(root / 'state'), CUMULUS_RENDERER=mode)
        result = subprocess.run([str(args.binary.resolve()), '--cumulus-renderer-diagnostics'],
                                env=env, capture_output=True, text=True, timeout=60)
        (root / f'{index}.log').write_text(result.stderr, encoding='utf-8')
        assert result.returncode == 0, f'graphics check failed ({mode}): {result.stdout} {result.stderr}'
        report = json.loads(result.stdout)
        (root / f'{index}.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        assert report['selected'] and report['real_music_requests'] == 0
        assert report['attempts'][-1]['outcome'] == 'ready'
        if index == 1:
            assert report['cached'] == reports[0]['selected']
            assert len(report['attempts']) == 1
        if mode != 'auto':
            assert report['selected'] == mode and len(report['attempts']) == 1
        reports.append(report)
    assert not list(root.rglob('*.sqlite3')), 'renderer check initialized business state'
    print(json.dumps({'ok': True, 'actualStartupRenderer': True, 'checks': reports}))


if __name__ == '__main__':
    main()
