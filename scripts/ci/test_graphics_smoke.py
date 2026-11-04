import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from graphics_smoke import run_check


class GraphicsFailureEvidence(unittest.TestCase):
    def test_failing_renderer_keeps_the_structured_attempts_but_still_fails(self):
        report = {'schema': 1, 'selected': None, 'cached': None, 'real_music_requests': 0,
                  'attempts': [{'mode': 'auto', 'outcome': 'timed-out'},
                               {'mode': 'software', 'outcome': 'timed-out'}]}
        trace = b'renderer probe +1000ms: WebView created\n'
        result = subprocess.CompletedProcess([], 1, json.dumps(report).encode('utf-8'), trace)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch('graphics_smoke.subprocess.run', return_value=result):
                with self.assertRaisesRegex(AssertionError, 'failed .*timed-out'):
                    run_check(root / 'cumulus', root, 0, 'auto', {})
            self.assertEqual(json.loads((root / '0.json').read_text(encoding='utf-8')), report)
            self.assertEqual((root / '0.log').read_bytes(), trace)

    def test_launcher_timeout_preserves_partial_bytes_without_creating_a_report(self):
        stdout = b'{"schema":1,'
        stderr = b'probe still running \xff\n'
        timeout = subprocess.TimeoutExpired([], 60, output=stdout, stderr=stderr)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch('graphics_smoke.subprocess.run', side_effect=timeout):
                with self.assertRaisesRegex(AssertionError, 'timed out .*probe still running'):
                    run_check(root / 'cumulus', root, 0, 'software', {})
            self.assertEqual((root / '0.stdout.txt').read_bytes(), stdout)
            self.assertEqual((root / '0.log').read_bytes(), stderr)
            self.assertFalse((root / '0.json').exists())

    def test_success_status_cannot_hide_invalid_json_encoding(self):
        stdout = b'{"selected":"auto","invalid":"\xff"}'
        result = subprocess.CompletedProcess([], 0, stdout, b'')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch('graphics_smoke.subprocess.run', return_value=result):
                with self.assertRaisesRegex(AssertionError, 'invalid graphics report'):
                    run_check(root / 'cumulus', root, 0, 'auto', {})
            self.assertEqual((root / '0.stdout.txt').read_bytes(), stdout)
            self.assertFalse((root / '0.json').exists())
