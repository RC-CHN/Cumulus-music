"""Resolve the exact core library harness reported by Cargo."""

import json
import subprocess


def build_core_test_executable(log, *, repo):
    args = ['cargo', 'test', '--locked', '-p', 'cumulus-core', '--lib', '--no-run',
            '--message-format=json']
    result = subprocess.run(args, cwd=repo, capture_output=True, encoding='utf-8',
                            errors='replace', check=False, timeout=600)
    log.write(result.stderr)
    if result.returncode:
        log.write(result.stdout)
        raise RuntimeError(f'could not build core Rust test harness (exit {result.returncode})')
    log.flush()
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    paths = {
        (repo / item['executable']).resolve(strict=True)
        for item in artifacts
        if item.get('reason') == 'compiler-artifact' and item.get('executable')
        and item['target']['name'] == 'cumulus_core' and item['profile']['test']
    }
    if len(paths) != 1:
        raise ValueError('expected exactly one cargo-produced core test executable')
    return paths.pop()
