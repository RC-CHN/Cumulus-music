"""Archive only the allowlisted synthetic CI proofs, preserving unusual paths.

upload-artifact rejects colons even on Linux; our filter path-escaping fixtures
intentionally contain them. A tar preserves these names without renaming proof
inputs or losing the rest of a failed job's evidence.

Every selected core.sqlite3 must be verifiable and contain no device identity
records. Refuse the archive instead of silently removing leaked proof data.
"""
import argparse
from contextlib import closing
from heapq import nlargest
from io import BytesIO
import json
import os
from pathlib import Path, PurePosixPath
import re
import shlex
import sqlite3
import stat
import tarfile
import tempfile


REPOSITORY = Path(__file__).resolve().parents[2]

# Only these non-UI outputs accompany the workflow's explicit proof roots.
# Platform and report names are closed sets; a neighboring research directory
# must not enter the archive merely because its name starts with "ci-".
FIXED_PATHS = (
    'package-stage/linux-x64/equalizer-proof',
    'package-stage/windows-x64/equalizer-proof',
    # Linux's run_auth_probe takes its proof root positionally, without --root.
    'ci-login', 'ci-login.log',
    'ci-flac150-linux/report.json', 'ci-flac150-linux/build.log',
    'package-stage/linux-x64/flac-source-build/report.json',
    'package-stage/linux-x64/flac-source-build/build.log',
    'windows-media/build.log', 'windows-media/plan.json', 'windows-media/installed.json',
    'windows-media/player-probe.log', 'windows-media/player-probe.stdout.log', 'windows-media/player-probe.json', 'windows-media/equalizer-proof',
)
NATIVE_REPORTS = (
    'ci-native-audio-linux', 'ci-native-audio-windows',
    'ci-native-stereo-pcm-linux', 'ci-windows-cold-flac/stereo-pcm-gui',
    'ci-native-audio-loudness-linux', 'ci-native-audio-loudness-windows',
    'ci-native-audio-stereo-linux', 'ci-native-audio-stereo-windows',
)


def workflow_roots():
    """Read literal --root arguments; never execute a workflow or expand globs.

    A proof root must be a single named child of .build. This intentionally does
    not accept variable substitutions, parent paths or arbitrary repository data.
    """
    roots = set()
    for workflow in sorted((REPOSITORY / '.github/workflows').glob('*.yml')):
        for line in workflow.read_text(encoding='utf-8').splitlines():
            if '--root' not in line:
                continue
            words = shlex.split(line, comments=True)
            for index, word in enumerate(words[:-1]):
                if word != '--root':
                    continue
                path = PurePosixPath(words[index + 1])
                if (len(path.parts) == 2 and path.parts[0] == '.build'
                        and path.name and all(char.isascii() and (char.isalnum() or char in '-_')
                                              for char in path.name)):
                    roots.add(path.name)
    return roots


def proof_paths(build):
    names = set(FIXED_PATHS)
    for root in workflow_roots():
        names.update((root, root + '.log'))
    for report in NATIVE_REPORTS:
        names.add(report + '-profile')
        names.update(report + suffix for suffix in
                     ('.json', '.log', '.failure.json', '.progress.jsonl', '.pcm'))
    for platform in ('linux-x64', 'windows-x64'):
        names.update(f'ci-{probe}-roundtrip-{platform}.log' for probe in ('engine', 'equalizer', 'pinned-audio'))
    return sorted(build / name for name in names if (build / name).exists())


def verify_core_database(path, mode):
    """Reject device facts without reading or reporting an identity value.

    Use a read-only SQLite snapshot, including committed WAL records. Immutable
    mode would ignore a WAL and could incorrectly approve a leaking database.
    """
    if not stat.S_ISREG(mode):
        raise ValueError('Refusing diagnostics archive: proof core.sqlite3 could not be verified.')
    try:
        with closing(sqlite3.connect(path.resolve().as_uri() + '?mode=ro', uri=True)) as database:
            database.execute('PRAGMA query_only = ON')
            database.execute('BEGIN')
            if database.execute('PRAGMA quick_check').fetchall() != [('ok',)]:
                raise sqlite3.DatabaseError()
            identity = database.execute(
                'SELECT 1 FROM state WHERE owner = ? AND kind = ? LIMIT 1',
                ('device', 'identity'),
            ).fetchone()
    except (OSError, ValueError, sqlite3.Error):
        # SQLite errors can contain database-controlled text. Suppress both the
        # original message and its traceback chain rather than exposing values.
        raise ValueError('Refusing diagnostics archive: proof core.sqlite3 could not be verified.') from None
    if identity is not None:
        raise ValueError('Refusing diagnostics archive: proof core.sqlite3 contains device identity records.')


def verify_proof_databases(paths):
    """Walk exactly the archived roots; preserve tar's non-dereferenced links."""
    try:
        for path in paths:
            mode = path.lstat().st_mode
            if path.name == 'core.sqlite3':
                verify_core_database(path, mode)
            elif stat.S_ISDIR(mode):
                # Unlike rglob, an unreadable directory must not silently hide
                # an unverified core database from this pre-archive check.
                verify_proof_databases(path.iterdir())
    except (OSError, RecursionError):
        raise ValueError('Refusing diagnostics archive: proof databases could not be verified.') from None


def archive_member(member):
    # WebView2 can still hold this synchronization file open after the
    # inspected app exits. It has no evidence and can truncate the whole tar.
    if member.isfile() and PurePosixPath(member.name).parts[-2:] == ('EBWebView', 'lockfile'):
        return None
    return member


class FailureCheckpoints:
    """Index ordinary receipt names as tar visits the already allowed files."""

    _part = re.compile(r'[A-Za-z0-9_-]{1,96}')
    _checkpoint = re.compile(r'(ux_[0-9]{2,4}_[A-Za-z0-9_-]{1,80})\.(?:json|png)')
    _limit = 12

    def __init__(self):
        self.failures = set()
        self.latest = {}

    def visit(self, member):
        if not member.isfile():
            return
        path = PurePosixPath(member.name)
        # Display only regular proof receipts in named CI directories. Never
        # print arbitrary fixture names, receipt contents or platform paths.
        if (not 2 <= len(path.parts) <= 5 or not path.parts[0].startswith('ci-')
                or not all(self._part.fullmatch(part) for part in path.parts[:-1])):
            return
        if path.name == 'failed.json':
            self.failures.add(path.parent)
        elif match := self._checkpoint.fullmatch(path.name):
            label = match[1]
            rank = (int(label.split('_', 2)[1]), label)
            if rank > self.latest.get(path.parent, (0, '')):
                self.latest[path.parent] = rank

    def lines(self):
        for root in sorted(self.failures)[:self._limit]:
            previous = self.latest.get(root, (0, 'none'))[1]
            yield f'Failed proof: {root}/failed.json; last recorded checkpoint: {previous}'
        remaining = len(self.failures) - self._limit
        if remaining > 0:
            yield f'Additional failed proof receipts in the archive: {remaining}'


def archive(build: Path, destination: Path, *, checkpoints=None):
    paths = proof_paths(build)
    verify_proof_databases(paths)
    destination.parent.mkdir(parents=True, exist_ok=True)

    def include(member):
        member = archive_member(member)
        if member is not None and checkpoints is not None:
            checkpoints.visit(member)
        return member

    with tarfile.open(destination, 'w:gz', dereference=False) as output:
        for path in paths:
            output.add(path, arcname=path.relative_to(build).as_posix(), filter=include)
    return len(paths)


# These exact lanes use the sealed podcast probe's stage writer. Keep other
# diagnostics in the broad archive until their receipt producers are reviewed.
COMPACT_LANES = {
    'ci-voice-songs-windows': ('cover', 'double'),
    'ci-voice-songs-linux': ('cover', 'double'),
    'ci-voice-songs-dense-linux': ('dense',),
}
COMPACT_CHECKPOINT = re.compile(r'ux_([0-9]{2,4})_[A-Za-z0-9_-]{1,80}\.json')
COMPACT_NEIGHBORS = 3
MAX_RECEIPT_BYTES = 256 * 1024
MAX_COMPACT_BYTES = 4 * 1024 * 1024


def link_or_reparse(info):
    return stat.S_ISLNK(info.st_mode) or bool(
        getattr(info, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
    )


def plain_directory(path):
    try:
        info = path.lstat()
    except FileNotFoundError:
        return False
    if link_or_reparse(info) or not stat.S_ISDIR(info.st_mode):
        raise ValueError('Refusing compact diagnostics: invalid proof directory.')
    return True


def receipt_info(path):
    info = path.lstat()
    if link_or_reparse(info) or not stat.S_ISREG(info.st_mode) or info.st_nlink != 1:
        raise ValueError('Refusing compact diagnostics: invalid receipt file.')
    if not 0 < info.st_size <= MAX_RECEIPT_BYTES:
        raise ValueError('Refusing compact diagnostics: receipt size is out of bounds.')
    return info


def read_receipt(path, case):
    expected = receipt_info(path)
    flags = os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0)
    with os.fdopen(os.open(path, flags), 'rb') as stream:
        actual = os.fstat(stream.fileno())
        if (link_or_reparse(actual) or not stat.S_ISREG(actual.st_mode)
                or actual.st_nlink != 1 or not os.path.samestat(expected, actual)
                or expected.st_size != actual.st_size):
            raise ValueError('Refusing compact diagnostics: receipt changed before reading.')
        data = stream.read(MAX_RECEIPT_BYTES + 1)
    if len(data) != expected.st_size:
        raise ValueError('Refusing compact diagnostics: receipt changed while reading.')
    try:
        value = json.loads(data)
    except (ValueError, RecursionError):
        raise ValueError('Refusing compact diagnostics: invalid receipt JSON.') from None
    if (not isinstance(value, dict) or value.get('stage') != path.stem
            or value.get('mode') != case
            or (path.name == 'failed.json' and value.get('ok') is not False)):
        raise ValueError('Refusing compact diagnostics: unexpected receipt identity.')
    return data


def compact_archive(build):
    """Collect failed SongCard visits and their three newest sibling JSONs only."""
    try:
        return _compact_archive(build)
    except (OSError, RecursionError):
        raise ValueError('Refusing compact diagnostics: receipts could not be verified.') from None


def _compact_archive(build):
    if not plain_directory(build):
        return 0
    parent = build / 'diagnostics'
    destination = parent / 'failure-checkpoints.tar.gz'
    # Exclusive output prevents a failed/empty repeat from uploading stale bytes.
    if plain_directory(parent):
        try:
            destination.lstat()
        except FileNotFoundError:
            pass
        else:
            raise ValueError('Refusing compact diagnostics: archive already exists.')

    lanes = []
    for root in sorted(workflow_roots() & COMPACT_LANES.keys()):
        for case in COMPACT_LANES[root]:
            folder = build
            for part in (root, case, 'run', 'first'):
                folder /= part
                if not plain_directory(folder):
                    break
            else:
                try:
                    receipt_info(folder / 'failed.json')
                except FileNotFoundError:
                    continue
                lanes.append((folder, case))

    if not lanes:
        return 0
    # Identical real-data guard as the full archive, including committed WAL
    # data. Do not read receipts or create output until it passes.
    verify_proof_databases(proof_paths(build))

    members = {}
    total = 0
    for folder, case in lanes:
        neighbors = nlargest(COMPACT_NEIGHBORS, (
            (int(match[1]), path.name)
            for path in folder.iterdir()
            if (match := COMPACT_CHECKPOINT.fullmatch(path.name))
        ))
        for name in ['failed.json', *(name for _, name in reversed(neighbors))]:
            path = folder / name
            data = read_receipt(path, case)
            total += len(data)
            if total > MAX_COMPACT_BYTES:
                raise ValueError('Refusing compact diagnostics: total receipt size is out of bounds.')
            members[path.relative_to(build).as_posix()] = data

    parent.mkdir(exist_ok=True)
    plain_directory(parent)
    temp_path = None
    try:
        with tempfile.NamedTemporaryFile(dir=parent, delete=False) as temporary:
            temp_path = Path(temporary.name)
            with tarfile.open(fileobj=temporary, mode='w:gz', compresslevel=3) as output:
                for name, data in members.items():
                    member = tarfile.TarInfo(name)
                    member.size = len(data)
                    output.addfile(member, BytesIO(data))
        temp_path.replace(destination)
    finally:
        if temp_path is not None:
            temp_path.unlink(missing_ok=True)
    return len(members)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--failure-checkpoints', action='store_true',
                        help='archive only the bounded SongCard failure and nearby JSON receipts')
    if parser.parse_args().failure_checkpoints:
        count = compact_archive(Path('.build'))
        print(f'Archived {count} compact synthetic failure receipts.')
        return
    checkpoints = FailureCheckpoints()
    count = archive(Path('.build'), Path('.build/diagnostics/synthetic-proofs.tar.gz'),
                    checkpoints=checkpoints)
    print(f'Archived {count} synthetic CI proof roots (no private research paths).')
    for line in checkpoints.lines():
        print(line)


if __name__ == '__main__':
    main()
