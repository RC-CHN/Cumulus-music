"""Bind prebuilt Linux package probes to the current CI attempt and frontend."""

import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile

from cargo_core_test_harness import build_core_test_executable
from linux_webview_probe_artifact import (
    MAX_BINARY, MAX_RECEIPT, RECEIPT, binary_receipt, checked_size, frontend_digest, run_identity,
)


CORE = 'cumulus_core_media_tests'
SHARDS = {
    'media-decoder': (CORE, 'native_local_equalizer_probe'),
    'media-startup': (CORE, 'native_audio_probe', 'equalizer_ui_probe', 'loudness_ui_probe'),
}


def receipt(path, shard, frontend, identity):
    payload = {
        'kind': 'linux-packaged-media-probes-v1',
        **identity,
        'shard': shard,
        'frontendSha256': frontend,
        'binaries': [binary_receipt(path / name, name) for name in SHARDS[shard]],
    }
    return (json.dumps(payload, sort_keys=True) + '\n').encode()


def stage(artifact, examples, core, dist, identity):
    frontend = frontend_digest(dist)
    sources = {
        name: core if name == CORE else examples / name
        for names in SHARDS.values() for name in names
    }
    for source in sources.values():
        checked_size(source, MAX_BINARY)
        if not source.stat().st_mode & stat.S_IXUSR:
            raise ValueError(f'Linux packaged media probe is not executable: {source.name}')
    artifact.mkdir()
    for shard, names in SHARDS.items():
        output = artifact / shard
        output.mkdir()
        for name in names:
            shutil.copyfile(sources[name], output / name)
        (output / RECEIPT).write_bytes(receipt(output, shard, frontend, identity))


def restore(artifact, examples, shard, dist, identity):
    names = SHARDS[shard]
    if (not stat.S_ISDIR(artifact.lstat().st_mode)
            or {p.name for p in artifact.iterdir()} != {*names, RECEIPT}):
        raise ValueError('unexpected Linux packaged media artifact contents')
    manifest = artifact / RECEIPT
    checked_size(manifest, MAX_RECEIPT)
    for name in names:
        checked_size(artifact / name, MAX_BINARY)
    examples.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=examples) as temporary:
        copied = Path(temporary)
        for name in names:
            shutil.copyfile(artifact / name, copied / name)
        verified = receipt(copied, shard, frontend_digest(dist), identity)
        if manifest.read_bytes() != verified:
            raise ValueError('Linux packaged media receipt mismatch (checkout/run/attempt/shard/assets/binaries)')
        for name in names:
            (copied / name).chmod(0o755)
        installed = []
        try:
            for name in names:
                destination = examples / name
                os.link(copied / name, destination)
                installed.append(destination)
        except OSError:
            for destination in installed:
                destination.unlink()
            raise
    return verified


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('stage', 'restore'))
    parser.add_argument('--examples', required=True, type=Path)
    parser.add_argument('--frontend-dist', required=True, type=Path)
    parser.add_argument('--artifact', required=True, type=Path)
    parser.add_argument('--shard', choices=SHARDS)
    args = parser.parse_args()
    if (args.action == 'restore') != (args.shard is not None):
        parser.error('--shard is required only for restore')
    checkout = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    identity = run_identity(os.environ, checkout)
    if args.action == 'stage':
        core = build_core_test_executable(sys.stderr, repo=Path.cwd())
        if core.parent != (args.examples.parent / 'deps').resolve(strict=True):
            raise ValueError('core test executable is outside the examples build target')
        stage(args.artifact, args.examples, core, args.frontend_dist, identity)
    else:
        manifest = restore(args.artifact, args.examples, args.shard, args.frontend_dist, identity)
        print(manifest.decode(), end='')


if __name__ == '__main__':
    main()
