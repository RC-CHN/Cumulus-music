from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from queue_more_tip_smoke import LOG_LIMIT, MILESTONE_LIMIT, main, phase_failure_receipt


class PhaseFailureReceipt(unittest.TestCase):
    def test_first_stack_overflow_has_phase_specific_evidence_without_raw_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            first = root / 'first'
            first.mkdir()
            (first / 'ux_01_unfocused.json').write_text('private-payload', encoding='ascii')
            (first / 'ux_02_at_zero.json').write_text('private-payload', encoding='ascii')
            (first / 'ux_01_restart_suppressed.json').write_text('wrong-phase', encoding='ascii')
            (root / 'first.log').write_bytes(
                b'noise-with-paths\r\nqueue-tip-first:daily_ready\r\n'
                b'queue-tip-reopen:renderer_enter\r\nqueue-tip-first:renderer-controlled-value\r\n'
                b'queue-tip-first:capture_written_unfocused\r\nqueue-tip-first:capture_written_at_zero\r\n'
                b'queue-tip-first:capture_written_restart_suppressed\r\n'
                b'queue-tip-first:complete_written'
            )
            receipt = phase_failure_receipt(root, 'first', 3221225725, 'child_exit')
            frozen = json.loads((root / 'first-failure-receipt.json').read_text(encoding='utf-8'))
            self.assertEqual(receipt, frozen)
            self.assertEqual(receipt['phase'], 'first')
            self.assertEqual(receipt['windowsExitStatus'], '0xC00000FD')
            self.assertTrue(receipt['stackOverflow'])
            self.assertEqual(receipt['stageFilesPresent'], ['unfocused', 'at_zero'])
            self.assertFalse(receipt['completeWritten'] or receipt['failedWritten'])
            self.assertEqual(receipt['milestones'], [
                'daily_ready', 'capture_written_unfocused', 'capture_written_at_zero',
            ])
            self.assertEqual(receipt['lastMilestone'], 'capture_written_at_zero')
            for private in ('renderer-controlled-value', 'noise-with-paths', 'private-payload'):
                self.assertNotIn(private, json.dumps(receipt))
            self.assertFalse((root / 'first-failure-receipt.tmp').exists())
            self.assertFalse((root / 'reopen-failure-receipt.json').exists())

    def test_native_activation_and_controller_request_remain_distinct_from_dom_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'first.log').write_bytes(
                b'queue-tip-first:focus_enter\r\n'
                b'queue-tip-first:focus_foreground_observed\r\n'
                b'queue-tip-first:focus_controller_requested\r\n'
                b'queue-tip-first:focus_native_observed\r\n'
                b'queue-tip-first:focus_returned\r\n'
                b'queue-tip-first:focus_renderer_mismatch\r\n'
                b'queue-tip-first:renderer-controlled-focus-details\r\n'
            )
            receipt = phase_failure_receipt(root, 'first', 0, 'renderer_failure')
            self.assertEqual(receipt['milestones'], [
                'focus_enter', 'focus_foreground_observed', 'focus_controller_requested',
                'focus_native_observed', 'focus_returned', 'focus_renderer_mismatch',
            ])
            self.assertEqual(receipt['lastMilestone'], 'focus_renderer_mismatch')
            self.assertFalse(receipt['stackOverflow'])

    def test_reopen_signed_stack_overflow_preserves_post_capture_phase(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            reopen = root / 'reopen'
            reopen.mkdir()
            (reopen / 'ux_01_restart_suppressed.json').touch()
            (reopen / 'complete.json').touch()
            (root / 'reopen.log').write_bytes(
                b'queue-tip-reopen:capture_written_restart_suppressed\n'
                b'queue-tip-reopen:complete_written\nqueue-tip-reopen:shutdown_stop_enter\n'
            )
            receipt = phase_failure_receipt(root, 'reopen', -1073741571, 'child_exit')
            self.assertTrue(receipt['stackOverflow'])
            self.assertEqual(receipt['stageFilesPresent'], ['restart_suppressed'])
            self.assertTrue(receipt['completeWritten'])
            self.assertEqual(receipt['lastMilestone'], 'shutdown_stop_enter')
            self.assertTrue((root / 'reopen-failure-receipt.json').is_file())

    def test_receipt_bounds_noisy_stderr_and_does_not_invent_exit_status(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'reopen.log').write_bytes(
                b'queue-tip-reopen:main_page_loaded\n' * 100 + b'x' * LOG_LIMIT
            )
            receipt = phase_failure_receipt(root, 'reopen', None, 'launch_timeout')
            self.assertEqual(receipt['reason'], 'launch_timeout')
            self.assertIsNone(receipt['windowsExitStatus'])
            self.assertFalse(receipt['stackOverflow'])
            self.assertTrue(receipt['logReadTruncated'] and receipt['milestoneLimitReached'])
            self.assertEqual(receipt['milestones'], ['main_page_loaded'] * MILESTONE_LIMIT)
            self.assertEqual(receipt['stageFilesPresent'], [])
            self.assertFalse(receipt['completeWritten'] or receipt['failedWritten'])


    def test_first_child_crash_is_reported_and_remains_fatal_without_reopening(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp) / 'proof'

            def launch(_binary, env, log, output, screenshots):
                self.assertNotIn('CUMULUS_PROOF_REOPEN', env)
                self.assertFalse(screenshots)
                self.assertEqual(output, root / 'first')
                log.write('queue-tip-first:main_window_build_enter\n')
                return 3221225725

            console = io.StringIO()
            with (patch('queue_more_tip_smoke.sys.platform', 'win32'),
                  patch('queue_more_tip_smoke.sys.argv', ['queue-more-tip', '--binary', 'synthetic.exe',
                                                        '--root', str(root)]),
                  patch('queue_more_tip_smoke.launch', side_effect=launch) as attempt,
                  redirect_stdout(console),
                  self.assertRaisesRegex(AssertionError, 'first cue probe exited 3221225725')):
                main()
            attempt.assert_called_once()
            self.assertFalse((root / 'reopen.log').exists())
            self.assertFalse((root / 'audit.json').exists())
            self.assertEqual(json.loads(console.getvalue())['lastMilestone'], 'main_window_build_enter')
