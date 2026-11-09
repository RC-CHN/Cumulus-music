#!/usr/bin/env python3
"""Stage the selected databases of a stopped Linux installation, then clean up."""

import argparse
from contextlib import ExitStack, closing, contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import stat
import subprocess
import sys
import tempfile

DB = "core.sqlite3"
SIDECARS = tuple(DB + suffix for suffix in ("-wal", "-shm", "-journal"))
DB_LIMIT = 64 * 1024 * 1024
ROW_LIMIT = 32 * 1024
NO_FOLLOW = os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW
STAGING_ERRORS = (OSError, ValueError, sqlite3.Error, NotImplementedError, RecursionError)


class StageRefused(ValueError):
    pass


def require(condition):
    if not condition:
        raise StageRefused("private source snapshot was refused")


def absolute(path):
    path = Path(path)
    require(path.anchor == "/" and ".." not in path.parts)
    return path


def directory(stack, name, parent=None):
    fd = os.open(name, NO_FOLLOW | os.O_DIRECTORY, dir_fd=parent)
    stack.callback(os.close, fd)
    return fd


def absolute_directory(stack, path):
    # Never resolve a symlink or hand a source pathname to SQLite.
    with ExitStack() as intermediate:
        parent = directory(intermediate, "/")
        for component in path.parts[1:]:
            parent = directory(intermediate, component, parent)
        result = os.dup(parent)
        stack.callback(os.close, result)
        return result


def signature(info):
    # Exclude atime: opening a stopped database read-only can update it.
    return (info.st_dev, info.st_ino, info.st_mode, info.st_uid, info.st_gid,
            info.st_nlink, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def entry(parent, name):
    return signature(os.stat(name, dir_fd=parent, follow_symlinks=False))


def owned_directory(fd):
    info = os.fstat(fd)
    require(info.st_uid == os.geteuid())
    return signature(info)


def no_sidecars(parent):
    for name in SIDECARS:
        try:
            os.stat(name, dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            continue
        raise StageRefused("stopped source has a SQLite sidecar")


def private_file(stack, parent, name, bound):
    fd = os.open(name, NO_FOLLOW | os.O_NONBLOCK, dir_fd=parent)
    stack.callback(os.close, fd)
    info = os.fstat(fd)
    require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
            and stat.S_IMODE(info.st_mode) & 0o077 == 0 and info.st_nlink == 1
            and 0 < info.st_size <= bound)
    baseline = signature(info)
    require(entry(parent, name) == baseline)
    return fd, baseline, info.st_size


def scan(fd, size, output=None):
    digest = hashlib.sha256()
    for offset in range(0, size, 1024 * 1024):
        length = min(1024 * 1024, size - offset)
        data = os.pread(fd, length, offset)
        require(len(data) == length)
        if output is not None:
            output.write(data)
        digest.update(data)
    require(not os.pread(fd, 1, size))
    return digest.digest()


def write_private(path, write):
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, "wb") as output:
        os.fchmod(output.fileno(), 0o600)
        return write(output)


def make_private_directory(path):
    path.mkdir(mode=0o700)
    path.chmod(0o700)


class Database:
    def __init__(self, stack, parent, destination):
        self.parent = parent
        no_sidecars(parent)
        self.fd, self.baseline, self.size = private_file(stack, parent, DB, DB_LIMIT)
        header = os.pread(self.fd, 100, 0)
        require(len(header) == 100 and header.startswith(b"SQLite format 3\0")
                and header[18:20] in (b"\x01\x01", b"\x02\x02"))
        self.digest = write_private(destination, lambda output: scan(self.fd, self.size, output))
        self.recheck()

    def recheck(self):
        no_sidecars(self.parent)
        require(signature(os.fstat(self.fd)) == self.baseline
                and entry(self.parent, DB) == self.baseline
                and scan(self.fd, self.size) == self.digest
                and signature(os.fstat(self.fd)) == self.baseline
                and entry(self.parent, DB) == self.baseline)
        no_sidecars(self.parent)


def unique_object(pairs):
    result = dict(pairs)
    require(len(result) == len(pairs))
    return result


def row(database, kind, key):
    rows = database.execute(
        """SELECT value FROM state WHERE owner='transport' AND kind=? AND key=?
           AND typeof(value)='text' AND length(CAST(value AS BLOB))<=? LIMIT 2""",
        (kind, key, ROW_LIMIT),
    ).fetchall()
    require(len(rows) == 1)
    return json.loads(rows[0][0], object_pairs_hook=unique_object)


def selected_session(registry, uid):
    content = registry.read_bytes()
    # A fully checkpointed WAL database can retain the WAL-mode header.
    # Normalize only SQLite's memory copy, never the staged bytes.
    with closing(sqlite3.connect(":memory:")) as database:
        require(hasattr(database, "deserialize"))
        database.execute("PRAGMA trusted_schema=OFF")
        database.execute("PRAGMA temp_store=MEMORY")
        database.deserialize(content[:18] + b"\x01\x01" + content[20:])
        database.execute("PRAGMA query_only=ON")
        tables = database.execute("PRAGMA main.table_list('state')").fetchall()
        require(len(tables) == 1 and tables[0][:4] == ("main", "state", "table", 4))
        require(row(database, "profiles", "configuration") ==
                {"schema": 1, "origin": "https://interfacepc.music.163.com"})
        selected = row(database, "profiles", "active")
        require(type(selected) is dict and selected.get("auto_login", True) is True
                and all(isinstance(selected.get(field), str)
                        and re.fullmatch(r"[0-9a-f]{32}", selected[field])
                        for field in ("session_id", "revision")))
        session = selected["session_id"]
        # Bind the independently supplied UID to the single active session.
        # Do not fetch nickname, avatar, or the credential record.
        candidates = database.execute(
            """SELECT key,
                      json_extract(value,'$.uid'),
                      json_type(value,'$.auto_login'),
                      length(CAST(value AS BLOB)),
                      (SELECT count(*) FROM json_each(state.value)
                       WHERE key IN ('uid', 'session_id', 'auto_login'))
               FROM state
               WHERE owner='transport' AND kind='saved-profiles-v1'
                 AND CASE WHEN json_valid(value)
                     THEN json_extract(value,'$.session_id')=? ELSE 0 END
               LIMIT 2""",
            (session,),
        ).fetchall()
        require(len(candidates) == 1 and candidates[0][:3] == (uid, uid, "true")
                and candidates[0][3] <= ROW_LIMIT and candidates[0][4] == 3)
        return session


@contextmanager
def staged_profile(source, expected_uid_file, *, source_stopped=False):
    """Yield (private snapshot path, private expected UID file) to a trusted consumer."""
    require(sys.platform == "linux" and source_stopped is True)
    source, uid_file = absolute(source), absolute(expected_uid_file)
    scratch_parent = Path("/tmp")
    require(source != scratch_parent and source not in scratch_parent.parents
            and source != uid_file and source not in uid_file.parents)
    with ExitStack() as stack, tempfile.TemporaryDirectory(prefix="cws-", dir=scratch_parent) as temp:
        try:
            root = absolute_directory(stack, source)
            root_baseline = owned_directory(root)
            uid_parent = absolute_directory(stack, uid_file.parent)
            uid_fd, uid_baseline, uid_size = private_file(stack, uid_parent, uid_file.name, 21)
            uid = os.pread(uid_fd, uid_size, 0)
            require(len(uid) == uid_size and re.fullmatch(rb"[1-9][0-9]{0,19}\n?", uid))
            temp = Path(temp)
            temp.chmod(0o700)
            snapshot = temp / "source"
            snapshot_uid = temp / "uid"
            make_private_directory(snapshot)
            registry = Database(stack, root, snapshot / DB)
            session = selected_session(snapshot / DB, uid.removesuffix(b"\n").decode("ascii"))
            sessions = directory(stack, "sessions", root)
            sessions_baseline = owned_directory(sessions)
            selected = directory(stack, session, sessions)
            selected_baseline = owned_directory(selected)
            make_private_directory(snapshot / "sessions")
            make_private_directory(snapshot / "sessions" / session)
            credential = Database(stack, selected, snapshot / "sessions" / session / DB)
            write_private(snapshot_uid, lambda output: output.write(uid))

            def recheck():
                registry.recheck()
                credential.recheck()
                with ExitStack() as check:
                    current_root = absolute_directory(check, source)
                    current_uid_parent = absolute_directory(check, uid_file.parent)
                    require(signature(os.fstat(current_root)) == root_baseline
                            and entry(root, "sessions") == sessions_baseline
                            and entry(sessions, session) == selected_baseline
                            and signature(os.fstat(uid_fd)) == uid_baseline
                            and entry(current_uid_parent, uid_file.name) == uid_baseline
                            and os.pread(uid_fd, uid_size + 1, 0) == uid
                            and signature(os.fstat(uid_fd)) == uid_baseline)

            recheck()
        except STAGING_ERRORS:
            raise StageRefused("private source snapshot was refused") from None
        try:
            yield snapshot, snapshot_uid
        finally:
            try:
                recheck()
            except STAGING_ERRORS:
                raise StageRefused("stopped source changed during the proof") from None


def main():
    parser = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--expected-uid-file", type=Path, required=True)
    parser.add_argument("--source-stopped", action="store_true", required=True)
    parser.add_argument("proof_args", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not args.proof_args or args.proof_args[0] != "--":
        parser.error("pass the live proof arguments after --")
    proof_args = args.proof_args[1:]
    if "--authorize-readonly" not in proof_args:
        parser.error("explicit read-only authorization is required after --")
    if any(arg.split("=", 1)[0] in ("--", "--source", "--expected-uid-file", "--source-stopped")
           for arg in proof_args):
        parser.error("source, UID file and stopped-source assertion belong before --")
    try:
        with staged_profile(args.source, args.expected_uid_file,
                            source_stopped=args.source_stopped) as (snapshot, uid_file):
            launcher = Path(__file__).with_name("signed_window_live_proof.py")
            # The fixed supervisor still requires its own explicit read-only
            # authorization and handles network/profile isolation itself.
            result = subprocess.run(
                [sys.executable, launcher, *proof_args, "--source", snapshot,
                 "--expected-uid-file", uid_file, "--source-stopped"],
                stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL, check=False,
            )
        if result.returncode:
            print("signed window supervisor refused; inspect its sanitized result if present",
                  file=sys.stderr)
        raise SystemExit(result.returncode)
    except STAGING_ERRORS:
        raise SystemExit("private staging refused; no source data was reported") from None


if __name__ == "__main__":
    main()
