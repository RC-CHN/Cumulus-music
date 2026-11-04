from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import unittest

from diagnostics import verify_proof_databases
from loudness_ui_smoke import read_closed_loudness_backups


BACKUP = '{"current":{"enabled":"broken"}}'


class LoudnessAuditDatabaseTests(unittest.TestCase):
    def damaged_probe_exit(self, path, *, identity=False):
        with sqlite3.connect(path) as db:
            db.execute('CREATE TABLE state(owner TEXT, kind TEXT, key TEXT, value TEXT)')
            db.execute('INSERT INTO state VALUES (?, ?, ?, ?)',
                       ('local-device', 'loudness-recovery-v1', 'backup', BACKUP))
            if identity:
                db.execute('INSERT INTO state VALUES (?, ?, ?, ?)',
                           ('device', 'identity', 'fake', '{}'))
        db.close()
        # Spill dirty pages and leave a real hot DELETE-mode journal behind.
        # The final UPDATE must never turn into a valid audit receipt.
        subprocess.run([sys.executable, '-c', '''
import os, sqlite3, sys
db = sqlite3.connect(sys.argv[1])
db.execute('PRAGMA synchronous=FULL')
db.execute('PRAGMA cache_size=1')
db.execute('BEGIN IMMEDIATE')
db.execute('UPDATE state SET value=? WHERE owner=?', ('x' * 524288, 'local-device'))
os._exit(0)
''', str(path)], check=True, timeout=10)
        journal = path.with_name(path.name+'-journal')
        self.assertTrue(journal.is_file())
        return journal

    def test_closed_probe_recovers_committed_backup_and_original_is_verifiable(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'core.sqlite3'
            journal = self.damaged_probe_exit(path)
            with self.assertRaisesRegex(ValueError, 'could not be verified'):
                verify_proof_databases([path])
            self.assertEqual(read_closed_loudness_backups(path), [(BACKUP,)])
            self.assertFalse(journal.exists())
            verify_proof_databases([path])

    def test_recovery_does_not_hide_a_committed_device_identity_from_diagnostics(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'core.sqlite3'
            self.damaged_probe_exit(path, identity=True)
            self.assertEqual(read_closed_loudness_backups(path), [(BACKUP,)])
            with self.assertRaisesRegex(ValueError, 'contains device identity'):
                verify_proof_databases([path])

    def test_missing_database_fails_without_creating_a_replacement(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'core.sqlite3'
            with self.assertRaises(sqlite3.OperationalError):
                read_closed_loudness_backups(path)
            self.assertFalse(path.exists())
