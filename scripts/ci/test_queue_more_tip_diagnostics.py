import json
from pathlib import Path
import tempfile
import unittest

from queue_more_tip_smoke import REOPEN_LOG_LIMIT, reopen_failure_receipt


class ReopenFailureReceipt(unittest.TestCase):
    def test_stack_overflow_preserves_post_capture_phase_without_raw_log_content(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            reopen = root / 'reopen'
            reopen.mkdir()
            (reopen / 'ux_01_restart_suppressed.json').write_text('{}', encoding='ascii')
            (reopen / 'complete.json').write_text('{}', encoding='ascii')
            (root / 'reopen.log').write_bytes(
                b'noise-with-paths\r\nqueue-tip-reopen:daily_ready\r\n'
                b'queue-tip-reopen:renderer-controlled-value\r\n'
                b'queue-tip-reopen:complete_written\r\nqueue-tip-reopen:shutdown_stop_enter\r\n'
            )
            receipt = reopen_failure_receipt(root, -1073741571)
            self.assertEqual(receipt, json.loads((root / 'reopen-failure-receipt.json').read_text(encoding='utf-8')))
            self.assertEqual(receipt['windowsExitStatus'], '0xC00000FD')
            self.assertTrue(receipt['stackOverflow'])
            self.assertTrue(receipt['stageWritten'] and receipt['completeWritten'])
            self.assertFalse(receipt['failedWritten'])
            self.assertEqual(receipt['milestones'], ['daily_ready', 'complete_written', 'shutdown_stop_enter'])
            self.assertNotIn('renderer-controlled-value', json.dumps(receipt))
            self.assertNotIn('noise-with-paths', json.dumps(receipt))

    def test_receipt_bounds_a_noisy_or_reentrant_stderr_without_guessing_stack_overflow(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'reopen.log').write_bytes(b'queue-tip-reopen:main_page_loaded\n' * 100 + b'x' * REOPEN_LOG_LIMIT)
            receipt = reopen_failure_receipt(root, 1)
            self.assertEqual(receipt['windowsExitStatus'], '0x00000001')
            self.assertFalse(receipt['stackOverflow'])
            self.assertTrue(receipt['logReadTruncated'] and receipt['milestoneLimitReached'])
            self.assertEqual(receipt['milestones'], ['main_page_loaded'] * 48)
            self.assertFalse(receipt['stageWritten'] or receipt['completeWritten'] or receipt['failedWritten'])
