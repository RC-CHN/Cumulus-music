"""Native process failures retain evidence and still fail the CI job."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from native_smoke import run_native, read_native_report


class NativeDiagnostics(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.report = Path(self.directory.name) / 'proof.json'
        self.env = {**os.environ, 'PYTHONIOENCODING': 'utf-8'}

    def run_fixture(self, code, **kwargs):
        with contextlib.redirect_stdout(io.StringIO()):
            run_native([sys.executable, '-c', code], self.env, self.report, **kwargs)

    def test_success_retains_log_without_fabricating_a_native_proof(self):
        self.run_fixture("print('synthetic completion')")
        self.assertIn('synthetic completion', self.report.with_suffix('.log').read_text(encoding='utf-8'))
        self.assertFalse(self.report.exists())
        self.assertFalse(self.report.with_suffix('.failure.json').exists())

    def test_zero_exit_without_required_report_retains_actual_failure_stage(self):
        self.run_fixture("print('synthetic PCM failure, despite zero exit')")
        self.report.with_suffix('.progress.jsonl').write_text(
            '{"stage":"window_created"}\n{"stage":"verification_failed"}\n', encoding='utf-8')
        with contextlib.redirect_stdout(io.StringIO()) as output, self.assertRaises(AssertionError):
            read_native_report(self.report)
        self.assertIn('synthetic PCM failure', output.getvalue())
        failure = json.loads(self.report.with_suffix('.failure.json').read_text(encoding='utf-8'))
        self.assertEqual(failure['kind'], 'missing_report')
        self.assertEqual(failure['lastStage'], 'verification_failed')
        self.assertFalse(self.report.exists())

    def test_completed_report_is_read_without_replacing_native_evidence(self):
        proof = {'ok': True, 'syntheticFixture': 'complete'}
        self.report.write_text(json.dumps(proof), encoding='utf-8')
        self.assertEqual(read_native_report(self.report), proof)
        self.assertFalse(self.report.with_suffix('.failure.json').exists())

    def test_failed_process_keeps_utf8_output_and_original_exit_status(self):
        with self.assertRaises(subprocess.CalledProcessError) as caught:
            self.run_fixture("import sys; print('合成失败', file=sys.stderr); sys.exit(7)")
        self.assertEqual(caught.exception.returncode, 7)
        value = json.loads(self.report.with_suffix('.failure.json').read_text(encoding='utf-8'))
        self.assertEqual((value['ok'], value['kind'], value['returnCode']), (False, 'exit', 7))
        self.assertIn('合成失败', self.report.with_suffix('.log').read_text(encoding='utf-8'))

    def test_timeout_keeps_failure_evidence_and_does_not_retry(self):
        with self.assertRaises(subprocess.TimeoutExpired):
            self.run_fixture('import time; time.sleep(30)', timeout=.05)
        value = json.loads(self.report.with_suffix('.failure.json').read_text(encoding='utf-8'))
        self.assertEqual((value['ok'], value['kind'], value['timeoutSeconds']), (False, 'timeout', .05))
        self.assertTrue(self.report.with_suffix('.log').exists())

    def test_missing_executable_is_not_reported_as_timeout_or_success(self):
        with contextlib.redirect_stdout(io.StringIO()), self.assertRaises(OSError):
            run_native([str(self.report.parent / 'absent-executable')], self.env, self.report)
        value = json.loads(self.report.with_suffix('.failure.json').read_text(encoding='utf-8'))
        self.assertFalse(value['ok'])
        self.assertEqual(value['kind'], 'spawn')


if __name__ == '__main__':
    unittest.main()
