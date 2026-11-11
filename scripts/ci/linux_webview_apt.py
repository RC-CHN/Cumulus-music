"""Bounded acquisition and optional same-run reuse for signed Ubuntu dependencies."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile
import time

from linux_webview_apt_mirrors import (
    DIRECT_HTTPS_REFRESH, AptDeadline, acquire, configured_source, deadline_command, direct_sources, preflight,
    verify_fresh_indexes,
)
from linux_webview_probe_artifact import run_identity


# Playback resolves the versioned libmpv at runtime; CI does not need its
# headers, pkg-config file or unversioned linker symlink.
COMMON_PACKAGES = (
    'libwebkit2gtk-4.1-dev', 'libxdo-dev', 'libayatana-appindicator3-dev', 'librsvg2-dev',
    'patchelf', 'libmpv2', 'ffmpeg', 'xvfb', 'dbus-x11', 'python3-gi', 'gir1.2-gtk-3.0',
    'libxtst6', 'openbox', 'x11-utils', 'iproute2',
)
ACCESSIBILITY_PACKAGES = ('gir1.2-atspi-2.0', 'at-spi2-core')
WEBVIEW_PACKAGES = (*COMMON_PACKAGES, 'openssl')
NATIVE_PACKAGES = (*COMMON_PACKAGES, *ACCESSIBILITY_PACKAGES, 'pipewire', 'pipewire-pulse',
                   'wireplumber', 'pulseaudio-utils', 'desktop-file-utils')
PACKAGE_PROFILES = {
    'webview': WEBVIEW_PACKAGES,
    'local-webview': (*WEBVIEW_PACKAGES, *ACCESSIBILITY_PACKAGES),
    'native': NATIVE_PACKAGES,
    'package': (*NATIVE_PACKAGES, 'cmake', 'binutils'),
}
OPTIONAL_SPEECH_MODEL = 'pocketsphinx-en-us'
MAX_FILES = 1024
MAX_BYTES = 512 * 1024 * 1024
MAX_DEB = 128 * 1024 * 1024
MAX_RECEIPT = 1024
MAX_PLAN = 1024 * 1024
BLOCK = 1024 * 1024
DEB_NAME = r'[a-zA-Z0-9][a-zA-Z0-9._+%~\-]{0,249}\.deb'
URI_ROW = re.compile(rf"'[^'\s]+' ({DEB_NAME}) ([1-9][0-9]{{0,8}}) SHA256:([0-9a-f]{{64}})")
READ_FLAGS = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
# These indexes feed package descriptions, software centers and shell command
# hints. Dependency resolution still uses every signed deb Packages index.
DIRECT_INDEX_OPTIONS = (
    '-o', 'Acquire::Languages=none',
    *(arg for target in ('DEP-11', 'DEP-11-icons-small', 'DEP-11-icons', 'DEP-11-icons-hidpi',
                          'DEP-11-icons-large', 'DEP-11-icons-large-hidpi', 'CNF')
      for arg in ('-o', f'Acquire::IndexTargets::deb::{target}::DefaultEnabled=false')),
)


def receipt(identity):
    return (json.dumps({'kind': 'linux-webview-apt-cache-v1', **identity}, sort_keys=True) + '\n').encode()


def apt_plan(data):
    """APT's exact install plan, from fresh authenticated indexes and an empty cache."""
    if len(data) > MAX_PLAN:
        raise ValueError('APT plan exceeds the size bound')
    rows = {}
    total = 0
    for line in data.decode('utf-8').splitlines():
        if not line.startswith("'"):
            continue  # apt-get prints the normal dependency summary, too
        match = URI_ROW.fullmatch(line)
        if not match:
            raise ValueError('APT package row does not have a safe name, size and SHA256')
        name, byte_count, sha256 = match.groups()
        size = int(byte_count)
        total += size
        if name in rows or len(rows) == MAX_FILES or size > MAX_DEB or total > MAX_BYTES:
            raise ValueError('duplicate or oversized APT plan')
        rows[name] = (size, sha256)
    return rows


def cache_file(directory_fd, name):
    """Open without following a symlink or blocking on an unexpected FIFO."""
    handle = os.open(name, READ_FLAGS, dir_fd=directory_fd)
    try:
        info = os.fstat(handle)
        if not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
            raise ValueError('not a regular cache file')
        return os.fdopen(handle, 'rb'), info.st_size
    except BaseException:
        os.close(handle)
        raise


def copy_verified(directory_fd, name, expected, staging):
    size, sha256 = expected
    target = staging / name
    try:
        file, actual = cache_file(directory_fd, name)
        with file:
            if actual != size:
                return False
            with target.open('xb') as output:
                remaining = size
                digest = hashlib.sha256()
                while remaining:
                    block = file.read(min(BLOCK, remaining))
                    if not block:
                        break
                    remaining -= len(block)
                    digest.update(block)
                    output.write(block)
                if not remaining and not file.read(1) and digest.hexdigest() == sha256:
                    return True
    except (OSError, ValueError):
        pass  # A missing or unreadable entry is fetched normally by APT.
    target.unlink(missing_ok=True)
    return False


def admit(source, identity, plan, staging):
    """A producer receipt identifies a run; it never authorizes package bytes."""
    try:
        directory_fd = os.open(source, READ_FLAGS | os.O_DIRECTORY)
    except OSError:
        return []
    try:
        try:
            file, size = cache_file(directory_fd, 'receipt.json')
            with file:
                if not 0 < size <= MAX_RECEIPT or file.read(MAX_RECEIPT + 1) != receipt(identity):
                    return []
        except (OSError, ValueError):
            return []
        return [staging / name for name, expected in plan.items()
                if copy_verified(directory_fd, name, expected, staging)]
    finally:
        os.close(directory_fd)


def stage(archive_dir, output, identity):
    """Retain archives already acquired by the producer's required APT transaction."""
    with tempfile.TemporaryDirectory(dir=output.parent) as scratch:
        prepared = Path(scratch) / 'apt-cache'
        prepared.mkdir()
        total = count = 0
        for path in archive_dir.iterdir():
            if not re.fullmatch(DEB_NAME, path.name):
                continue
            info = path.lstat()
            count += 1
            total += info.st_size
            if (not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= MAX_DEB
                    or count > MAX_FILES or total > MAX_BYTES):
                raise ValueError('unusable producer APT archive set')
            shutil.copyfile(path, prepared / path.name)
        if count:
            (prepared / 'receipt.json').write_bytes(receipt(identity))
            prepared.rename(output)
        return count, total


def timed(label, args, env, limit=None, **kwargs):
    start = time.monotonic()
    try:
        command = deadline_command(args, limit) if limit is not None else args
        return subprocess.run(command, check=True, env=env, **kwargs)
    except subprocess.CalledProcessError as error:
        # timeout returns 124, or 137 after forcibly terminating its group.
        # An early unrelated SIGKILL is not an acquisition deadline.
        if limit is not None and (error.returncode == 124 or (
                error.returncode == 137 and time.monotonic() - start >= limit)):
            raise AptDeadline(label, limit) from error
        raise
    finally:
        print(f'APT {label}: {time.monotonic() - start:.3f}s', flush=True)


def restore_for_plan(source, identity, apt, request, env, archives, workspace):
    start = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='verified-', dir=workspace) as private:
        staging = Path(private)
        plan_file = workspace / 'fresh-plan'
        try:
            # Noble otherwise defaults print-uris to MD5. APT must see an
            # empty cache to emit every archive selected by the new indexes.
            with plan_file.open('wb') as output:
                timed('SHA256 plan', [*apt, '-o', 'Acquire::ForceHash=SHA256',
                      '--print-uris', '--download-only', *request], env, stdout=output)
            with plan_file.open('rb') as file:
                plan = apt_plan(file.read(MAX_PLAN + 1))
            accepted = admit(source, identity, plan, staging)
        except (OSError, ValueError, subprocess.CalledProcessError) as error:
            print(f'APT archive reuse omitted: {error}', flush=True)
            accepted = []
        if accepted:
            # APT treats existing final cache entries as complete by size.
            # Only independently verified private copies enter.
            subprocess.run(['sudo', 'install', '-m', '0644', '-t', archives,
                            '--', *accepted], check=True)
        total = sum(path.stat().st_size for path in accepted)
        print(f'APT archive reuse: {len(accepted)} signed matches, {total} bytes; '
              f'{time.monotonic() - start:.3f}s including the plan; fetch all misses', flush=True)


def package_request(profile, supported_runner, env):
    request = ['install', '-y', *PACKAGE_PROFILES[profile]]
    if not supported_runner:
        return request
    try:
        state = subprocess.run(['dpkg-query', '--show', '--showformat=${db:Status-Status}\n',
                                OPTIONAL_SPEECH_MODEL], env=env, capture_output=True, text=True)
    except OSError:
        return request
    absent = (state.returncode == 0 and state.stdout == 'not-installed\n' and not state.stderr
              or state.returncode == 1 and not state.stdout and state.stderr ==
              f'dpkg-query: no packages found matching {OPTIONAL_SPEECH_MODEL}\n')
    if absent:
        # This data package is only a recommendation of the retained runtime
        # library. For an absent package APT's exact '-' marker prevents a
        # new install; --no-remove rejects any removal if state changed.
        print(f'APT dependency plan: omit absent optional {OPTIONAL_SPEECH_MODEL}; no removals', flush=True)
        request.extend(('--no-remove', f'{OPTIONAL_SPEECH_MODEL}-'))
    return request


def install(args):
    env = {**os.environ, 'LC_ALL': 'C'}
    checkout = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    identity = run_identity(env, checkout)
    with tempfile.TemporaryDirectory(prefix='cumulus-webview-apt-', dir='/tmp') as work:
        workspace = Path(work)
        workspace.chmod(0o755)  # the _apt sandbox must be able to traverse it
        archives = workspace / 'archives'
        subprocess.run(['sudo', 'install', '-d', '-m', '0755', '--', archives], check=True)
        apt = ['sudo', 'apt-get', '-o', f'Dir::Cache::archives={archives}']
        source = configured_source(env)
        request = package_request(args.profile, source is not None, env)
        sources = {}
        private_lists = []

        def transaction(mirror):
            if mirror is None:
                return apt, None
            if mirror.uri not in sources:
                variant = workspace / f'mirror-{len(sources)}'
                variant.mkdir(mode=0o755)
                parts, lists = variant / 'sources.list.d', variant / 'lists'
                direct_sources(source, parts, mirror.uri)
                private_lists.append(lists)
                subprocess.run(['sudo', 'install', '-d', '-m', '0755', '--', lists], check=True)
                sources[mirror.uri] = ([*apt, '-o', f'Dir::Etc::sourceparts={parts}',
                    '-o', f'Dir::State::lists={lists}', '-o', 'Dir::Cache::pkgcache=',
                    '-o', 'Dir::Cache::srcpkgcache=', *DIRECT_INDEX_OPTIONS], lists)
            return sources[mirror.uri]

        def update(mirror, limit):
            # Do not derive authority from any artifact or an index update
            # that silently used stale metadata after a transport failure.
            selected, lists = transaction(mirror)
            refresh = DIRECT_HTTPS_REFRESH if mirror is not None else ()
            timed('update', [*selected, *refresh, '-o', 'APT::Update::Error-Mode=any', 'update'], env, limit)
            if mirror is not None:
                verify_fresh_indexes(mirror, lists)

        def prepare(mirror):
            if args.reuse:
                selected, _ = transaction(mirror)
                restore_for_plan(args.reuse, identity, selected, request, env, archives, workspace)

        def download(mirror, limit):
            selected, _ = transaction(mirror)
            timed('archive acquisition', [*selected, '--download-only', *request], env, limit)

        def reset():
            subprocess.run(['sudo', 'rm', '-rf', '--', archives], check=True)
            subprocess.run(['sudo', 'install', '-d', '-m', '0755', '--', archives], check=True)

        try:
            routes = preflight() if source is not None else None
            if routes is None:
                print('APT mirror preflight: runner preset unsupported; normal APT acquisition', flush=True)
            with acquire(update, prepare, download, reset, routes) as (mirror, downloaded):
                if args.publish:
                    if not downloaded:
                        download(mirror, None)
                    try:
                        count, total = stage(archives, args.publish, identity)
                        print(f'APT archive handoff: {count} files, {total} bytes', flush=True)
                    except (OSError, ValueError) as error:
                        print(f'APT archive handoff omitted: {error}', flush=True)
                # Don't put a network watchdog around any dpkg operation.
                selected, _ = transaction(mirror)
                timed('signed installation', [*selected, *request], env)
        finally:
            # APT owns these dedicated cache/partial directories only.
            subprocess.run(['sudo', 'rm', '-rf', '--', archives, *private_lists], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=PACKAGE_PROFILES, default='webview')
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--publish', type=Path)
    mode.add_argument('--reuse', type=Path)
    mode.add_argument('--cold', action='store_true', help='install without a producer or shared cache')
    install(parser.parse_args())


if __name__ == '__main__':
    main()
