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

import windows_native_open_smoke as proof


class WindowsNativeOpenEvidence(unittest.TestCase):
    def test_success_and_interrupted_app_export_only_evidence(self):
        for interrupted in (False, True):
            with self.subTest(interrupted=interrupted), tempfile.TemporaryDirectory() as outer:
                root = Path(outer)
                evidence = root / 'ci-native-open-windows'
                encoder = root / 'ffmpeg.exe'
                encoder.touch()
                identity = dict(commit='a' * 40, ciRun='17', ciAttempt='2')
                observed = []

                def package(packages, commit, work, **options):
                    self.assertEqual(packages, root)
                    self.assertEqual(commit, identity['commit'])
                    self.assertEqual(options, dict(ci_run='17', ci_attempt='2', require_package_flac=True))
                    runtime = work / 'portable'
                    runtime.mkdir()
                    (runtime / 'cumulus.exe').write_bytes(b'fixture, never executed')
                    return runtime

                def verify(work, target, binary, fixture_encoder, env):
                    self.assertEqual(target, evidence)
                    self.assertNotIn(evidence, work.parents)
                    self.assertNotIn(evidence, binary.parents)
                    self.assertEqual(fixture_encoder, encoder)
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

                with patch.object(proof, 'require_hosted_windows', return_value=identity), \
                        patch.object(proof, 'portable_runtime', side_effect=package), \
                        patch.object(proof, 'block_app_network', return_value=nullcontext()), \
                        patch.object(proof, 'verify_app', side_effect=verify), \
                        patch.dict(os.environ, {'SystemRoot': str(root), 'CUMULUS_SESSION_DIR': 'ambient'}), \
                        patch.object(sys, 'argv', ['proof', '--packages', str(root),
                                                  '--root', str(evidence), '--fixture-encoder', str(encoder)]), \
                        redirect_stdout(io.StringIO()):
                    if interrupted:
                        with self.assertRaisesRegex(AssertionError, 'interrupted native app'):
                            proof.main()
                    else:
                        proof.main()
                work, = observed
                self.assertFalse(work.exists())
                expected = {'owner.log', 'sender.log'}
                if not interrupted:
                    expected.add('report.json')
                    report = json.loads((evidence / 'report.json').read_text(encoding='utf-8'))
                    self.assertTrue(report['ok'])
                    self.assertEqual({k: report[k] for k in identity}, identity)
                self.assertEqual({path.name for path in evidence.iterdir()}, expected)
