import json
from pathlib import Path
import unittest
from unittest.mock import patch
from snapshot_read import read_snapshot


class SnapshotRead(unittest.TestCase):
    def test_replacement_race_reads_the_actual_next_snapshot(self):
        with patch.object(Path, 'read_text', side_effect=[PermissionError('busy'), FileNotFoundError('replacing'), '{', '{"generation":4}']), patch('snapshot_read.time.sleep'):
            self.assertEqual(read_snapshot(Path('synthetic')), {'generation': 4})

    def test_persistent_busy_missing_or_malformed_snapshot_still_fails(self):
        for failure in [PermissionError('busy'), FileNotFoundError('missing'), json.JSONDecodeError('bad', '{', 1)]:
            with self.subTest(failure=type(failure).__name__), patch.object(Path, 'read_text', side_effect=failure), patch('snapshot_read.time.monotonic', side_effect=[0, 3]):
                with self.assertRaises(type(failure)):
                    read_snapshot(Path('synthetic'))

    def test_other_io_failures_are_not_retried(self):
        with patch.object(Path, 'read_text', side_effect=OSError('device failed')) as read:
            with self.assertRaises(OSError):
                read_snapshot(Path('synthetic'))
            self.assertEqual(read.call_count, 1)
