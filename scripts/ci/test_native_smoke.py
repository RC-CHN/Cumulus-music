"""Native process failures retain evidence and still fail the CI job."""
import contextlib
import copy
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from native_smoke import run_native, read_native_report, verify_stereo_native


def stereo_report_fixture():
    """Validator-only fixture; never execution evidence for native playback."""
    blocks = [{'phase': 'source', 'reset': index == 9,
               'revision': 1 if index < 3 else 2 if index < 5 else 3 if index == 5 else 4,
               'inputFrames': 64, 'outputFrames': 64, 'end': False,
               'validPcmSha256': 'a' * 64} for index in range(20)]
    remaining = 48000
    while remaining:
        frames = min(remaining, 2048)
        remaining -= frames
        blocks.append({'phase': 'eof', 'inputFrames': 0, 'outputFrames': frames,
                       'end': remaining == 0, 'validPcmSha256': 'b' * 64})
    return {'stereoNativeBoundary': True, 'syntheticMediaOnly': True,
            'nativeNullOutputOnly': True, 'remoteRequests': 0, 'accountWrites': 0,
            'productionStereoAttachment': False, 'wholeMaterialVerified': False,
            'physicalAudioOutputVerified': False,
            'dynamicComponent': {'ok': True, 'realDynamicLibrary': True,
                'versionCheckedByHost': 1, 'validPcmBitIdenticalToCore': True,
                'configurationRevoked': True, 'retainedInstanceMuted': True,
                'reopenedFreshInstance': True, 'sampleRate': 48000, 'channels': 2,
                'capacity': 2048, 'librarySha256': 'c' * 64, 'blocks': blocks,
                'tailFrames': 48000, 'tailNonzero': True, 'completeEof': True,
                'finalStats': {'revision': 4, 'observed_revision': 4,
                    'instances_created': 2, 'active_instances': 1,
                    'failed_blocks': 0, 'resets': 1, 'input_frames': 1280,
                    'output_frames': 49280}},
            'actualDecodedFormats': {'ok': True, 'monoPreserved': True,
                'outputDeviceCoercionUsed': False, 'staleEntryRejected': True,
                'stoppedEntryRejected': True,
                'cases': [{'channels': channels, 'expectedChannels': channels,
                           'entryId': channels, 'sampleRate': 48000,
                           'decoder': 'pcm_s16le', 'output': 'null',
                           'actualPosition': .2, 'nativeStereoQualification': channels == 2}
                          for channels in [1, 2]],
                'observedFormatQualification': {'instances_created': 1,
                    'active_instances': 0, 'observed_revision': 0,
                    'input_frames': 0, 'output_frames': 0}}}


class StereoBoundaryReport(unittest.TestCase):
    def test_complete_boundary_report_is_validated_without_promoting_it_to_attachment(self):
        verify_stereo_native(stereo_report_fixture())

    def test_tail_cannot_be_shortened_even_if_native_flags_and_totals_claim_success(self):
        proof = stereo_report_fixture()
        proof['dynamicComponent']['blocks'][-1]['outputFrames'] -= 1
        proof['dynamicComponent']['tailFrames'] -= 1
        proof['dynamicComponent']['finalStats']['output_frames'] -= 1
        with self.assertRaises(AssertionError):
            verify_stereo_native(proof)

    def test_mono_cannot_be_reported_as_stereo_or_qualified_from_output_layout(self):
        proof = stereo_report_fixture()
        for forged in ['channels', 'nativeStereoQualification']:
            case = copy.deepcopy(proof)
            case['actualDecodedFormats']['cases'][0][forged] = 2 if forged == 'channels' else True
            with self.assertRaises(AssertionError):
                verify_stereo_native(case)

    def test_component_and_separate_decoder_proof_cannot_claim_complete_live_attachment(self):
        for flag in ['productionStereoAttachment', 'wholeMaterialVerified', 'physicalAudioOutputVerified']:
            proof = stereo_report_fixture()
            proof[flag] = True
            with self.assertRaises(AssertionError):
                verify_stereo_native(proof)


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
