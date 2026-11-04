"""Exercise the shipped graphics-only startup path, with no account initialization."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def run_check(binary, root, index, mode, env):
    stderr = root / f'{index}.log'
    stdout = root / f'{index}.stdout.txt'
    try:
        result = subprocess.run([str(binary), '--cumulus-renderer-diagnostics'], env=env,
                                capture_output=True, timeout=60)
    except subprocess.TimeoutExpired as error:
        stderr.write_bytes(error.stderr or b'')
        stdout.write_bytes(error.stdout or b'')
        detail = stderr.read_text(encoding='utf-8', errors='replace')
        raise AssertionError(f'graphics check timed out ({mode}): {detail}') from error
    stderr.write_bytes(result.stderr)
    try:
        report = json.loads(result.stdout.decode('utf-8'))
    except ValueError:
        stdout.write_bytes(result.stdout)
        report = None
    else:
        # Preserve the ordinary structured all-attempts report even when no
        # renderer was selected and the diagnostic exited with an error.
        (root / f'{index}.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    detail = ' '.join(value.decode('utf-8', errors='replace') for value in (result.stdout, result.stderr))
    assert result.returncode == 0, f'graphics check failed ({mode}): {detail}'
    assert isinstance(report, dict), f'invalid graphics report ({mode}): {detail}'
    return report


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
        report = run_check(args.binary.resolve(), root, index, mode, env)
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
