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

from native_smoke import (STEREO_PCM_STAGES, run_native, read_native_report,
                          verify_stereo_native, verify_stereo_pcm)


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


class StereoPcmContract(unittest.TestCase):
    """Synthetic validator inputs are not native playback evidence."""

    def proof(self):
        return {
            'ok': True, 'kind': 'stereo_pcm_fixed_local_native_v2',
            'graphScope': 'complete_fixed_local_equalizer', 'fixedLocalMaterial': True,
            'generalSdkGraph': False, 'settingWriteApplyOrderVerified': False,
            'productionDesktopStartup': True, 'numericLocaleCompatible': True, 'guiInitialized': True,
            'actualDecoder': True, 'unknownLength': True, 'strictNcm': True,
            'syntheticDownloadPublished': True, 'syntheticMediaTransfers': 1,
            'workerJoinedBeforeSeek': True, 'sourcePathReplaced': True, 'reopenRejected': True,
            'actualNativeEof': True, 'revokedReclaimDenied': True,
            'fullSdkGraph': False, 'wholeMaterial': False, 'officialUiTested': False,
            'physicalAudioOutputVerified': False, 'realMusicRequests': 0, 'coldHttpAttempts': 0,
            'format': {'rate': 44100, 'channels': 2, 'bitsPerSample': 16,
                       'maxBlockFrames': 1024, 'sourceFrames': 88207},
            'stages': list(STEREO_PCM_STAGES), 'sameControlEntry': 11,
            'nativeDecodedFormat': {'rate': 44100, 'channels': 2, 'entry': 11},
            'requestedControls': [1, 2], 'positionBefore': .2, 'positionAfter': .3,
            'paused': {'nativeEntry': 11, 'paused': True, 'graphEnd': False, 'failed': False,
                       'nativeOutputFrame': 0, 'decodedSourceFrame': 8192, 'producedFrames': 7973,
                       'requestedRevision': 1, 'appliedRevision': 1},
            'live': {'nativeEntry': 11, 'failed': False, 'nativeOutputFrame': 10000,
                     'producedFrames': 20000, 'requestedRevision': 2, 'appliedRevision': 2},
            'retired': {'nativeEntry': 11, 'readerClosed': True, 'nativeSinkRetired': True},
            'seek': {'oldEntry': 11, 'newEntry': 22, 'firstSourceFrame': 22050,
                     'paused': True, 'sameHeldSource': True},
            'eof': {'nativeEntry': 22, 'graphEnd': True, 'readerClosed': True,
                    'nativeSinkRetired': True, 'producedFrames': 110257,
                    'decodedSourceFrame': 88207, 'nativeSourceFrame': 88207},
            'tailFrames': 44100,
            'revoked': {'nativeEntry': 33, 'readerClosed': True,
                        'nativeSinkRetired': True, 'cancelled': True},
            'revokedEntry': 33, 'bufferedAheadAtRevocation': 3000,
        }

    def rejected(self, change):
        proof = copy.deepcopy(self.proof())
        change(proof)
        with self.assertRaises((AssertionError, KeyError)):
            verify_stereo_pcm(proof)

    def test_complete_synthetic_schema_is_accepted(self):
        verify_stereo_pcm(self.proof())

    def test_no_gui_no_native_eof_or_no_join_cannot_pass(self):
        for key in ['guiInitialized', 'productionDesktopStartup', 'actualNativeEof',
                    'workerJoinedBeforeSeek', 'strictNcm', 'unknownLength']:
            with self.subTest(key=key):
                self.rejected(lambda p: p.update({key: False}))

    def test_fixed_scope_cannot_claim_arbitrary_sdk_setting_workflow_ui_or_physical_audio(self):
        for key in ['fullSdkGraph', 'wholeMaterial', 'generalSdkGraph', 'settingWriteApplyOrderVerified',
                    'officialUiTested', 'physicalAudioOutputVerified']:
            with self.subTest(key=key):
                self.rejected(lambda p: p.update({key: True}))

    def test_old_two_node_report_or_missing_fixed_material_claim_is_not_upgraded(self):
        self.rejected(lambda p: p.update(kind='stereo_pcm_native_v1'))
        self.rejected(lambda p: p.update(subgraph='SE1->defaultB0'))
        self.rejected(lambda p: p.update(graphScope='SE1->defaultB0'))
        self.rejected(lambda p: p.update(fixedLocalMaterial=False))

    def test_stale_entry_unapplied_control_and_unbounded_render_lead_fail(self):
        self.rejected(lambda p: p['live'].update(nativeEntry=12))
        self.rejected(lambda p: p['live'].update(appliedRevision=1))
        self.rejected(lambda p: p['live'].update(producedFrames=88207))
        self.rejected(lambda p: p['paused'].update(decodedSourceFrame=88207))
        self.rejected(lambda p: p['nativeDecodedFormat'].update(channels=1))

    def test_close_without_native_retirement_or_bad_seek_source_fail(self):
        self.rejected(lambda p: p['retired'].update(nativeSinkRetired=False))
        self.rejected(lambda p: p['seek'].update(sameHeldSource=False))
        self.rejected(lambda p: p['seek'].update(newEntry=11))
        self.rejected(lambda p: p['seek'].update(firstSourceFrame=0))

    def test_tail_source_clock_and_buffered_revocation_are_required(self):
        self.rejected(lambda p: p['eof'].update(producedFrames=110256))
        self.rejected(lambda p: p['eof'].update(nativeSourceFrame=110257))
        self.rejected(lambda p: p.update(bufferedAheadAtRevocation=0))
        self.rejected(lambda p: p['revoked'].update(cancelled=False))

    def test_missing_stage_or_actual_remote_attempt_cannot_pass(self):
        self.rejected(lambda p: p['stages'].pop())
        self.rejected(lambda p: p.update(coldHttpAttempts=1))
        self.rejected(lambda p: p.update(realMusicRequests=1))


if __name__ == '__main__':
    unittest.main()
