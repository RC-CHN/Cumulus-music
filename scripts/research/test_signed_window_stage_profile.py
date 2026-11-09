"""Stopped source staging tests with synthetic SQLite state only."""

from contextlib import closing
import json
import os
from pathlib import Path
import sqlite3
import stat
from subprocess import CompletedProcess
import tempfile
import unittest
from unittest.mock import patch

from signed_window_stage_profile import DB, SIDECARS, StageRefused, main, staged_profile

UID = "4107"
SESSION = "a" * 32
OTHER_SESSION = "b" * 32


def database(path, records, *, wal=False):
    with closing(sqlite3.connect(path)) as db:
        if wal:
            db.execute("PRAGMA journal_mode=WAL")
        db.execute("CREATE TABLE state(owner TEXT, kind TEXT, key TEXT, value TEXT, PRIMARY KEY(owner,kind,key))")
        db.execute("CREATE INDEX example_v1 ON state(owner,json_extract(value,'$.number')) WHERE kind='synthetic'")
        db.executemany("INSERT INTO state VALUES ('transport',?,?,?)",
                       ((kind, key, json.dumps(value)) for kind, key, value in records))
        db.commit()
    path.chmod(0o600)


class StagedProfileTests(unittest.TestCase):
    def setUp(self):
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.parent = Path(temp.name)
        self.source = self.parent / "installation"
        self.source.mkdir()
        self.source.chmod(0o775)
        self.selected = self.source / "sessions" / SESSION
        self.selected.mkdir(parents=True)
        self.other = self.source / "sessions" / OTHER_SESSION
        self.other.mkdir()
        self.uid = self.parent / "approved-uid"
        self.uid.write_text(f"{UID}\n", encoding="ascii")
        self.uid.chmod(0o600)
        self.registry()
        database(self.selected / DB, [("synthetic", "selected", {"number": 1})], wal=True)
        database(self.other / DB, [("synthetic", "different", {"number": 2})])

    def registry(self, *, session=SESSION, saved_uid=UID, duplicate=False):
        self.source.joinpath(DB).unlink(missing_ok=True)
        records = [
            ("profiles", "configuration", {"schema": 1, "origin": "https://interfacepc.music.163.com"}),
            ("profiles", "active", {"session_id": session, "revision": "1" * 32}),
            ("saved-profiles-v1", saved_uid, {"session_id": session, "uid": saved_uid, "auto_login": True}),
        ]
        if duplicate:
            records.append(("saved-profiles-v1", "14", {"session_id": session, "uid": "14", "auto_login": True}))
        database(self.source / DB, records, wal=True)

    def stage(self, source=None, uid=None):
        return staged_profile(source or self.source, uid or self.uid, source_stopped=True)

    def test_closed_wal_bytes_stay_unchanged_and_only_selected_files_are_staged(self):
        original = {DB: self.source.joinpath(DB).read_bytes(),
                    f"sessions/{SESSION}/{DB}": self.selected.joinpath(DB).read_bytes()}
        self.assertEqual(original[DB][18:20], b"\x02\x02")
        with self.stage() as (snapshot, uid):
            self.assertEqual(stat.S_IMODE(snapshot.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(snapshot.parent.stat().st_mode), 0o700)
            self.assertEqual(stat.S_IMODE(uid.stat().st_mode), 0o600)
            self.assertEqual(uid.read_bytes(), self.uid.read_bytes())
            self.assertEqual({str(path.relative_to(snapshot)) for path in snapshot.rglob("*") if path.is_file()},
                             original.keys())
            for relative, content in original.items():
                self.assertEqual(snapshot.joinpath(relative).read_bytes(), content)
                self.assertEqual(stat.S_IMODE(snapshot.joinpath(relative).stat().st_mode), 0o600)
            self.assertEqual([path.name for path in snapshot.joinpath("sessions").iterdir()], [SESSION])
        self.assertFalse(snapshot.parent.exists())
        self.assertEqual(stat.S_IMODE(self.source.stat().st_mode), 0o775)
        for relative, content in original.items():
            self.assertEqual(self.source.joinpath(relative).read_bytes(), content)
        self.assertFalse(any(self.source.rglob("*-wal")))

    def test_consumer_failure_removes_snapshot(self):
        with self.assertRaisesRegex(RuntimeError, "consumer failed"):
            with self.stage() as (snapshot, _):
                raise RuntimeError("consumer failed")
        self.assertFalse(snapshot.parent.exists())

    def test_cli_consumer_gets_only_staged_paths_and_preserves_exit_status(self):
        staged = []

        def consume(command, **_):
            self.assertEqual(command[1].name, "signed_window_live_proof.py")
            self.assertIn("--authorize-readonly", command)
            self.assertNotIn(self.source, command)
            self.assertNotIn(self.uid, command)
            snapshot = command[command.index("--source") + 1]
            uid = command[command.index("--expected-uid-file") + 1]
            staged.append(snapshot)
            self.assertTrue(snapshot.joinpath(DB).is_file())
            self.assertEqual(uid.read_bytes(), self.uid.read_bytes())
            return CompletedProcess(command, 17)

        argv = ["staging", "--source", str(self.source), "--expected-uid-file", str(self.uid),
                "--source-stopped", "--", "--authorize-readonly"]
        with patch("sys.argv", argv), patch("signed_window_stage_profile.subprocess.run", consume):
            with patch("sys.stderr"), self.assertRaises(SystemExit) as result:
                main()
        self.assertEqual(result.exception.code, 17)
        self.assertFalse(staged[0].parent.exists())

    def test_stop_assertion_and_independent_exact_uid_are_required(self):
        with self.assertRaises(StageRefused):
            with staged_profile(self.source, self.uid):
                self.fail("unconfirmed stopped source passed")
        self.uid.write_text("999\n", encoding="ascii")
        with self.assertRaises(StageRefused):
            with self.stage():
                self.fail("another account passed")
        self.uid.write_text(f"{UID}\n", encoding="ascii")
        self.registry(duplicate=True)
        with self.assertRaises(StageRefused):
            with self.stage():
                self.fail("ambiguous saved account passed")

    def test_untrusted_directory_and_database_links_and_permissions_are_refused(self):
        alias = self.parent / "alias"
        alias.symlink_to(self.source, target_is_directory=True)
        with self.assertRaises(StageRefused):
            with self.stage(source=alias):
                self.fail("symlinked root passed")
        real_db = self.selected / DB
        real_db.chmod(0o640)
        with self.assertRaises(StageRefused):
            with self.stage():
                self.fail("group-readable credential database passed")
        real_db.chmod(0o600)
        linked = self.parent / "linked-db"
        os.link(real_db, linked)
        with self.assertRaises(StageRefused):
            with self.stage():
                self.fail("multiply linked credential database passed")
        linked.unlink()
        real_db.rename(linked)
        real_db.symlink_to(linked)
        with self.assertRaises(StageRefused):
            with self.stage():
                self.fail("symlinked credential database passed")

    def test_both_source_directories_refuse_every_sidecar_including_dangling_symlinks(self):
        for parent in (self.source, self.selected):
            for name in SIDECARS:
                with self.subTest(parent=parent.name, name=name):
                    sidecar = parent / name
                    sidecar.symlink_to("not-present")
                    with self.assertRaises(StageRefused):
                        with self.stage():
                            self.fail("sidecar passed")
                    sidecar.unlink()

    def test_postcondition_rejects_sidecar_created_and_deleted_while_consumer_runs(self):
        with self.assertRaisesRegex(StageRefused, "source changed"):
            with self.stage() as (snapshot, _):
                sidecar = self.selected / SIDECARS[0]
                sidecar.touch()
                sidecar.unlink()
        self.assertFalse(snapshot.parent.exists())

    def test_postcondition_rejects_database_content_changed_in_place(self):
        with self.assertRaisesRegex(StageRefused, "source changed"):
            with self.stage() as (snapshot, _):
                with self.selected.joinpath(DB).open("r+b") as fd:
                    fd.seek(200)
                    fd.write(b"changed")
        self.assertFalse(snapshot.parent.exists())


if __name__ == "__main__":
    unittest.main()
