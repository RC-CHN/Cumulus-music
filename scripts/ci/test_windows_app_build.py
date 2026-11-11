"""Only a verified same-attempt Windows app may reach packaging."""

import json
from pathlib import Path
import tempfile
import unittest

from test_windows_package_probes import synthetic_pe
from windows_app_build import (
    ARTIFACT, BUNDLE_CONFIG, EXE, MANIFEST, MAX_CONFIG, MAX_MANIFEST, NULL_PROOF_ENV, null_proof_enabled,
    stage, verify,
)
from windows_flac import FLAC_DLL_SHA256
from windows_webview_inputs import FRONTEND, ci_identity


class WindowsAppBuildTest(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.repo = Path(scratch.name)
        self.identity = ci_identity(dict(GITHUB_SHA='a' * 40, GITHUB_RUN_ID='123', GITHUB_RUN_ATTEMPT='2'))
        self.target_dir = self.repo / 'alternate-target'
        self.executable = self.target_dir / 'release' / EXE
        self.executable.parent.mkdir(parents=True)
        self.executable.write_bytes(synthetic_pe(EXE))
        self.frontend = self.repo / FRONTEND
        self.asset = self.frontend / 'assets/app.js'
        self.asset.parent.mkdir(parents=True)
        self.asset.write_bytes(b'asset')
        (self.frontend / 'index.html').write_bytes(b'<title>fixture</title>')
        self.artifact = self.repo / ARTIFACT
        self.producer_config = self.repo / BUNDLE_CONFIG
        self.consumer_config = self.repo / 'consumer' / BUNDLE_CONFIG
        for config in (self.producer_config, self.consumer_config):
            config.parent.mkdir(parents=True)
            config.write_bytes(b'{"bundle":{"active":true}}')

    def stage(self, *, null_proof=False):
        stage(self.repo, self.identity, target_dir=self.target_dir, null_proof=null_proof)

    def verify(self, *, identity=None, null_proof=False):
        return verify(self.artifact, identity or self.identity, self.frontend,
                      null_proof=null_proof, config=self.consumer_config)

    def test_matching_build_and_frontend_return_verified_executable(self):
        self.stage(null_proof=True)
        self.assertEqual(self.verify(null_proof=True), self.artifact / EXE)
        self.assertEqual((self.artifact / EXE).read_bytes(), self.executable.read_bytes())
        manifest = json.loads((self.artifact / MANIFEST).read_bytes())
        self.assertEqual(manifest['flacSha256'], FLAC_DLL_SHA256)
        self.assertIs(manifest['nativeOpenNullProof'], True)
        self.assertEqual(set(manifest['frontend']), {'index.html', 'assets/app.js'})

    def test_consumer_config_must_match_exact_producer_bytes(self):
        self.stage()
        self.consumer_config.write_bytes(b'{"bundle":{"active":null}}')
        with self.assertRaisesRegex(AssertionError, 'checksum'):
            self.verify()
        self.consumer_config.write_bytes(b' ' * (MAX_CONFIG + 1))
        with self.assertRaisesRegex(AssertionError, 'invalid Windows bundle config'):
            self.verify()

    def test_producer_rejects_config_over_bound_before_staging(self):
        self.producer_config.write_bytes(b' ' * (MAX_CONFIG + 1))
        with self.assertRaisesRegex(AssertionError, 'invalid Windows bundle config'):
            self.stage()
        self.assertFalse(self.artifact.exists())

    def test_wrong_checkout_run_attempt_or_proof_mode_cannot_be_packaged(self):
        self.stage()
        for key, wrong in (('commit', 'b' * 40), ('ciRun', '124'), ('ciAttempt', '1')):
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, 'provenance'):
                self.verify(identity=self.identity | {key: wrong})
        with self.assertRaisesRegex(AssertionError, 'mode'):
            self.verify(null_proof=True)
        self.assertTrue(null_proof_enabled({NULL_PROOF_ENV: '1'}))
        for value in ('', '0', 'true', '01'):
            self.assertFalse(null_proof_enabled({NULL_PROOF_ENV: value}))
        self.assertFalse(null_proof_enabled({}))

    def test_same_size_binary_or_frontend_replacement_and_extra_assets_are_rejected(self):
        self.stage()
        for file in (self.artifact / EXE, self.asset):
            with self.subTest(file=file.name):
                original = file.read_bytes()
                file.write_bytes(original[:-1] + b'!')
                with self.assertRaisesRegex(AssertionError, 'checksum'):
                    self.verify()
                file.write_bytes(original)
        (self.frontend / 'assets/extra.js').write_bytes(b'unexpected')
        with self.assertRaisesRegex(AssertionError, 'checksum'):
            self.verify()

    def test_invalid_executable_is_rejected_before_staging(self):
        for options, reason in (({'machine': 0x14C}, 'x64'), ({'flags': 0x2002}, 'not a DLL')):
            with self.subTest(options=options):
                self.executable.write_bytes(synthetic_pe(EXE, **options))
                with self.assertRaisesRegex(AssertionError, reason):
                    self.stage()
                self.assertFalse(self.artifact.exists())

    def test_manifest_limits_digest_and_mode_type(self):
        self.stage()
        path = self.artifact / MANIFEST
        saved = json.loads(path.read_bytes())
        path.write_bytes(b' ' * (MAX_MANIFEST + 1))
        with self.assertRaisesRegex(AssertionError, 'invalid Windows release app manifest'):
            self.verify()
        path.write_text(json.dumps(saved | {'flacSha256': '0' * 64}), encoding='utf-8')
        with self.assertRaisesRegex(AssertionError, 'checksum'):
            self.verify()
        path.write_text(json.dumps(saved | {'nativeOpenNullProof': 0}), encoding='utf-8')
        with self.assertRaisesRegex(AssertionError, 'invalid native-open proof mode'):
            self.verify()


if __name__ == '__main__':
    unittest.main()
