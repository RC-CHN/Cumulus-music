"""Keep disposable app data out of the generic CI failure archive."""
from contextlib import closing, nullcontext, redirect_stdout
import io
import json
import os
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest
from unittest.mock import patch
import wave

import windows_native_open_smoke as proof


class WindowsNativeOpenEvidence(unittest.TestCase):
    def test_output_acceptance_and_interruption_export_only_evidence(self):
        for output, endpoint_kind, interrupted, retained in (
            ('default', 'endpoint_readable', False, False),
            ('default', 'default_endpoint_absent', False, False),
            ('null', 'default_endpoint_absent', False, False),
            ('null', 'default_endpoint_absent', True, False),
            ('null', 'default_endpoint_absent', False, True),
            ('null', 'default_endpoint_absent', True, True),
        ):
            with self.subTest(output=output, endpoint=endpoint_kind, interrupted=interrupted, retained=retained), \
                    tempfile.TemporaryDirectory() as outer:
                root = Path(outer)
                evidence = root / 'ci-native-open-windows'
                encoder = root / 'ffmpeg.exe'
                encoder.touch()
                endpoint_probe = root / 'endpoint.exe'
                endpoint_probe.touch()
                consumer = dict(commit='a' * 40, ciRun='17', ciAttempt='2')
                identity = proof.RETAINED_TRACE if retained else consumer
                package_asset = root / proof.RETAINED_ZIP
                package_asset.write_bytes(b'fixture ZIP, never extracted')
                binary_bytes = b'fixture, never executed'
                observed = []
                refused = output == 'default' and endpoint_kind != 'endpoint_readable'

                def package(packages, commit, work, **options):
                    self.assertEqual(packages, root)
                    self.assertEqual(commit, identity['commit'])
                    self.assertEqual(options, dict(ci_run=identity['ciRun'], ci_attempt=identity['ciAttempt'],
                                                   require_package_flac=True))
                    runtime = work / 'portable'
                    runtime.mkdir()
                    (runtime / 'cumulus.exe').write_bytes(binary_bytes)
                    return runtime

                def verify(work, target, binary, fixture_encoder, env, audio_output):
                    self.assertEqual(target, evidence)
                    self.assertNotIn(evidence, work.parents)
                    self.assertNotIn(evidence, binary.parents)
                    self.assertEqual(fixture_encoder, None if retained else encoder)
                    self.assertEqual(audio_output, output)
                    self.assertEqual(env.get('CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_OUTPUT'),
                                     'null' if output == 'null' else None)
                    self.assertEqual(env.get('CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_TRACE'),
                                     '1' if output == 'null' else None)
                    state = Path(env['CUMULUS_DATA_DIR'])
                    state.mkdir()
                    observed.append(work)
                    with closing(sqlite3.connect(state / 'core.sqlite3')) as db:
                        db.execute('CREATE TABLE state(owner,kind,key,value)')
                        db.commit()
                    (Path(env['WEBVIEW2_USER_DATA_FOLDER']) / 'browser-data').touch()
                    (work / 'synthetic.mp3').touch()
                    (target / 'owner.log').touch()
                    (target / 'sender.log').touch()
                    if interrupted:
                        raise AssertionError('interrupted native app')
                    return {'nativeRetainedReceipts': True}

                input_args = ['--retained-trace-package'] if retained else ['--fixture-encoder', str(encoder)]
                with patch.object(proof, 'require_hosted_windows', return_value=consumer), \
                        patch.object(proof, 'portable_runtime', side_effect=package), \
                        patch.object(proof, 'block_app_network', return_value=nullcontext()), \
                        patch.object(proof, 'collect_sample', return_value={'kind': endpoint_kind}), \
                        patch.object(proof, 'verify_app', side_effect=verify), \
                        patch.object(proof, 'RETAINED_ZIP_SHA', proof.file_sha(package_asset)), \
                        patch.object(proof, 'RETAINED_BINARY_SHA', proof.hashlib.sha256(binary_bytes).hexdigest()), \
                        patch.dict(os.environ, {'SystemRoot': str(root), 'CUMULUS_SESSION_DIR': 'ambient',
                                               'CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_OUTPUT': 'ambient',
                                               'CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_TRACE': 'ambient'}), \
                        patch.object(sys, 'argv', ['proof', '--packages', str(root),
                                                  '--root', str(evidence), *input_args,
                                                  '--endpoint-probe', str(endpoint_probe), '--audio-output', output]), \
                        redirect_stdout(io.StringIO()):
                    if interrupted:
                        with self.assertRaisesRegex(AssertionError, 'interrupted native app'):
                            proof.main()
                    elif refused:
                        with self.assertRaisesRegex(AssertionError, 'default_endpoint_absent'):
                            proof.main()
                    else:
                        proof.main()
                expected = {'report.json'}
                if refused:
                    self.assertEqual(observed, [])
                else:
                    work, = observed
                    self.assertFalse(work.exists())
                    expected.update(('owner.log', 'sender.log'))
                self.assertEqual({path.name for path in evidence.iterdir()}, expected)
                report = json.loads((evidence / 'report.json').read_text(encoding='utf-8'))
                self.assertEqual({k: report[k] for k in identity}, identity)
                self.assertEqual(report['proofAudioOutput'], output)
                self.assertEqual(report['defaultRenderEndpoint'], {'kind': endpoint_kind})
                self.assertEqual(report['ok'], not (refused or interrupted))
                self.assertEqual(report['defaultOutputClockVerified'],
                                 output == 'default' and not refused)
                self.assertFalse(report['physicalAudioOutputVerified'])
                self.assertEqual(report['failureStage'], 'default_render_endpoint_probe' if refused
                                 else 'native_file_activation' if interrupted else None)
                if retained:
                    self.assertEqual(report['proofCi'], consumer)
                    self.assertEqual(report['retainedPackageArtifact'], proof.RETAINED_ARTIFACT)
                    self.assertEqual(report['fixtureSource'], 'frozen-synthetic')
                    self.assertFalse(report['prerequisiteProofsRepeated'])
                    self.assertFalse(report['ordinaryWindowsPortableApp'])
                else:
                    self.assertNotIn('proofCi', report)

    def test_retained_diagnostic_rejects_default_or_altered_zip(self):
        with tempfile.TemporaryDirectory() as outer:
            root = Path(outer)
            (root / proof.RETAINED_ZIP).write_bytes(b'wrong retained package')
            for mode, error in (('default', SystemExit), ('null', AssertionError)):
                with self.subTest(mode=mode), \
                        patch.object(proof, 'require_hosted_windows', return_value=proof.RETAINED_TRACE), \
                        patch.object(proof, 'portable_runtime') as extract, \
                        patch.dict(os.environ, {'SystemRoot': str(root)}), \
                        patch.object(sys, 'argv', ['proof', '--packages', str(root), '--root', str(root / 'report'),
                                                  '--retained-trace-package', '--endpoint-probe', str(root / 'unused'),
                                                  '--audio-output', mode]), \
                        self.assertRaises(error):
                    proof.main()
                extract.assert_not_called()

    def test_retained_media_needs_no_encoder_and_keeps_picker_cases(self):
        with tempfile.TemporaryDirectory() as outer, patch.object(proof.subprocess, 'run') as run:
            cold, flac, mp3, invalid = proof.media_fixtures(Path(outer), None, {})
            run.assert_not_called()
            with wave.open(str(cold)) as pcm:
                self.assertEqual((pcm.getframerate(), pcm.getnchannels(), pcm.getnframes()),
                                 (44100, 2, 44100 * 24))
            self.assertEqual(proof.file_sha(flac), proof.FROZEN_WARM_AUDIO['warm.flac'])
            self.assertEqual(proof.file_sha(mp3), proof.FROZEN_WARM_AUDIO['warm.mp3'])
            self.assertEqual((cold.suffix, flac.suffix, mp3.suffix, invalid.suffix),
                             ('.WAV', '.flac', '.mp3', '.flac'))
            self.assertEqual(invalid.read_text(encoding='ascii'), 'https://example.invalid/not-a-media-file\n')
