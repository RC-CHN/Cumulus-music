#!/usr/bin/env python3
"""Run a desktop proof probe to completion and report its summary.

Generic driver: the probe owns the measurement logic and writes `complete.json`
or `failed.json` into the directory named by `CUMULUS_PROOF_DIR`. This script only
launches it, waits with a bounded timeout, and asserts the summary is ok.

Used to collect real WebView fingerprint references on each runner, including
Windows, where no shell launcher or screenshot helper exists.
"""
import argparse
import json
import os
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=180.0)
    args = parser.parse_args()

    root = args.root.resolve()
    if root.exists():
        raise SystemExit(f'proof directory must be new: {root}')
    root.parent.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env['CUMULUS_PROOF_DIR'] = str(root)
    binary = args.binary.resolve()

    log_path = root.parent / f'{root.name}.log'
    with log_path.open('w', encoding='utf-8') as log:
        process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + args.timeout
            while True:
                failed = root / 'failed.json'
                if failed.exists():
                    raise RuntimeError(failed.read_text(encoding='utf-8'))
                if (root / 'complete.json').exists():
                    break
                if process.poll() is not None:
                    raise RuntimeError(f'probe exited early with code {process.returncode}')
                if time.monotonic() > deadline:
                    raise RuntimeError(f'probe timed out after {args.timeout:.0f}s')
                time.sleep(0.1)
            if process.wait(timeout=30) != 0:
                raise RuntimeError(f'probe exited with code {process.returncode}')
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()

    report = json.loads((root / 'complete.json').read_text(encoding='utf-8'))
    assert report['ok'] is True, 'probe did not report a successful measurement'
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
