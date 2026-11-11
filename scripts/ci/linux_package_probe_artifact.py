"""Bind prebuilt Linux desktop probe sets to the current CI attempt and frontend."""

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
LOCAL_SHARDS = {
    'local-files-cloud': ('mini_ui_probe', 'local_ui_probe', 'local_batch_favorite_ui_probe'),
    'mini-player-lyrics': ('mini_ui_probe', 'desktop_lyrics_ui_probe'),
}
CATALOG_SHARDS = {
    'catalog-browse': ('catalog_ui_probe', 'discovery_home_ui_probe'),
    'catalog-community': ('catalog_ui_probe', 'comment_edit_ui_probe', 'community_ui_probe'),
}
SUITES = {
    'media': ('linux-packaged-media-probes-v1', SHARDS),
    'local-desktop': ('linux-local-desktop-probes-v1', LOCAL_SHARDS),
    'catalog': ('linux-catalog-probes-v1', CATALOG_SHARDS),
}


def receipt(path, shard, frontend, identity, *, suite='media'):
    kind, shards = SUITES[suite]
    payload = {
        'kind': kind,
        **identity,
        'shard': shard,
        'frontendSha256': frontend,
        'binaries': [binary_receipt(path / name, name) for name in shards[shard]],
    }
    return (json.dumps(payload, sort_keys=True) + '\n').encode()


def stage(artifact, examples, core, dist, identity, *, suite='media'):
    _, shards = SUITES[suite]
    frontend = frontend_digest(dist)
    sources = {
        name: core if name == CORE else examples / name
        for names in shards.values() for name in names
    }
    for source in sources.values():
        checked_size(source, MAX_BINARY)
        if not source.stat().st_mode & stat.S_IXUSR:
            raise ValueError(f'Linux desktop probe is not executable: {source.name}')
    artifact.mkdir()
    for shard, names in shards.items():
        output = artifact / shard
        output.mkdir()
        for name in names:
            shutil.copyfile(sources[name], output / name)
        (output / RECEIPT).write_bytes(receipt(output, shard, frontend, identity, suite=suite))


def restore(artifact, examples, shard, dist, identity, *, suite='media'):
    _, shards = SUITES[suite]
    names = shards[shard]
    if (not stat.S_ISDIR(artifact.lstat().st_mode)
            or {p.name for p in artifact.iterdir()} != {*names, RECEIPT}):
        raise ValueError('unexpected Linux desktop probe artifact contents')
    manifest = artifact / RECEIPT
    checked_size(manifest, MAX_RECEIPT)
    for name in names:
        checked_size(artifact / name, MAX_BINARY)
    examples.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=examples) as temporary:
        copied = Path(temporary)
        for name in names:
            shutil.copyfile(artifact / name, copied / name)
        verified = receipt(copied, shard, frontend_digest(dist), identity, suite=suite)
        if manifest.read_bytes() != verified:
            raise ValueError('Linux desktop probe receipt mismatch (checkout/run/attempt/suite/shard/assets/binaries)')
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
    parser.add_argument('--suite', choices=SUITES, default='media')
    parser.add_argument('--shard')
    args = parser.parse_args()
    if (args.action == 'restore') != (args.shard is not None):
        parser.error('--shard is required only for restore')
    if args.shard is not None and args.shard not in SUITES[args.suite][1]:
        parser.error('--shard is not part of the selected suite')
    checkout = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    identity = run_identity(os.environ, checkout)
    if args.action == 'stage':
        core = None
        if args.suite == 'media':
            core = build_core_test_executable(sys.stderr, repo=Path.cwd())
            if core.parent != (args.examples.parent / 'deps').resolve(strict=True):
                raise ValueError('core test executable is outside the examples build target')
        stage(args.artifact, args.examples, core, args.frontend_dist, identity, suite=args.suite)
    else:
        manifest = restore(args.artifact, args.examples, args.shard, args.frontend_dist, identity,
                           suite=args.suite)
        print(manifest.decode(), end='')


if __name__ == '__main__':
    main()
