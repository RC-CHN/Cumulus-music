"""Verify narrow missed observations in the installed Linux probes."""
from contextlib import closing
from pathlib import Path
import sqlite3
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest

from installed_linux_audio import read_playback, stale_accessible_lookup, wait_for


class PlaybackObservation(unittest.TestCase):
    def test_real_writer_lock_is_a_missed_sample(self):
        with TemporaryDirectory() as directory:
            path = Path(directory) / 'core.sqlite3'
            with closing(sqlite3.connect(path)) as writer:
                writer.execute('CREATE TABLE state (owner, kind, key, value)')
                writer.execute('INSERT INTO state VALUES (?, ?, ?, ?)', (
                    'local-guest', 'playback', 'position', '{"seconds":1}',
                ))
                writer.commit()
                writer.execute('BEGIN EXCLUSIVE')
                missed = []

                def sample_then_unlock():
                    try:
                        return read_playback(path)
                    except sqlite3.OperationalError as error:
                        missed.append(error.sqlite_errorcode)
                        writer.commit()
                        raise

                self.assertEqual(wait_for('position', sample_then_unlock, 3),
                                 {'position': {'seconds': 1}})
                self.assertEqual(missed, [sqlite3.SQLITE_BUSY])

                writer.execute('BEGIN EXCLUSIVE')
                with self.assertRaisesRegex(AssertionError, 'unavailable SQLite samples'):
                    wait_for('position', lambda: read_playback(path), .4)
                writer.commit()
                writer.execute('DROP TABLE state')
                with self.assertRaises(sqlite3.OperationalError) as invalid:
                    wait_for('position', lambda: read_playback(path), 3)
                self.assertEqual(invalid.exception.sqlite_errorcode, sqlite3.SQLITE_ERROR)

class AccessibilityObservation(unittest.TestCase):
    def test_disappeared_application_and_accessible_are_missed_samples(self):
        def error(code, message, domain='atspi_error'):
            return SimpleNamespace(domain=domain, code=code, message=message)

        lost_app = 'The application no longer exists'
        lost_object = ('No such interface “org.a11y.atspi.Accessible” on object at path '
                       '/org/a11y/atspi/accessible/23c6da36_025a_4ef2_a444_531e89e24899')
        self.assertTrue(stale_accessible_lookup(error(0, lost_app)))
        self.assertTrue(stale_accessible_lookup(error(1, lost_object)))
        for unexpected in (
            error(1, lost_app),
            error(0, lost_object),
            error(1, lost_object, domain='g-dbus-error-quark'),
            error(1, lost_object.replace('atspi.Accessible', 'atspi.Component')),
            error(1, lost_object.replace('/org/a11y/atspi/accessible/', '/unrelated/')),
            error(1, 'Timeout was reached'),
        ):
            self.assertFalse(stale_accessible_lookup(unexpected))
