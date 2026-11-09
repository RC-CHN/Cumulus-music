"""The Windows WebView runners only consume their package job's inputs."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from windows_webview_inputs import (
    ARTIFACT, DIST, FRONTEND, NATIVE, PACKAGES, UX, ci_identity, restore, stage, verify,
)


class WindowsWebViewInputs(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Path(directory.name)
        self.producer = self.repo / 'producer'
        self.consumer = self.repo / 'consumer'
        self.identity = ci_identity(dict(GITHUB_SHA='a' * 40, GITHUB_RUN_ID='123', GITHUB_RUN_ATTEMPT='2'))
        for path, content in ((FRONTEND / 'index.html', b'<title>synthetic</title>'),
                              (FRONTEND / 'assets/runtime.js', b'asset'),
                              (UX, b'package probe')):
            path = self.producer / path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        stage(self.producer, self.identity)
        self.artifact = self.consumer / ARTIFACT
        self.artifact.parent.mkdir(parents=True)
        (self.producer / ARTIFACT).rename(self.artifact)

    def test_rejects_another_commit_run_or_attempt_before_runtime_extraction(self):
        for field, replacement in (('commit', 'b' * 40), ('ciRun', '124'), ('ciAttempt', '1')):
            with self.subTest(field=field), patch('windows_webview_inputs.portable_runtime') as extract:
                with self.assertRaisesRegex(AssertionError, 'provenance'):
                    restore(self.consumer, self.identity | {field: replacement})
                extract.assert_not_called()
                self.assertFalse((self.consumer / FRONTEND).exists())

    def test_rejects_same_size_changed_binary_or_asset_and_extra_contents(self):
        for relative in (Path(UX.name), Path(DIST, 'assets/runtime.js')):
            with self.subTest(relative=relative):
                path = self.artifact / relative
                original = path.read_bytes()
                path.write_bytes(b'!' + original[1:])
                with self.assertRaisesRegex(AssertionError, 'checksum'):
                    verify(self.artifact, self.identity)
                path.write_bytes(original)
        (self.artifact / DIST / 'extra').write_bytes(b'extra')
        with self.assertRaisesRegex(AssertionError, 'checksum'):
            verify(self.artifact, self.identity)

    def test_restore_passes_exact_provenance_to_portable_verification_and_replaces_stale_probe(self):
        cached = self.consumer / UX
        cached.parent.mkdir(parents=True)
        cached.write_bytes(b'prior cached executable')

        def fixture_extract(packages, commit, temporary, **options):
            self.assertEqual(packages, self.consumer / PACKAGES)
            self.assertEqual(commit, self.identity['commit'])
            self.assertEqual(options, dict(ci_run='123', ci_attempt='2', require_package_flac=True))
            portable = temporary / 'synthetic'
            portable.mkdir()
            (portable / 'libmpv-2.dll').write_bytes(b'portable bytes')
            return portable

        with patch('windows_webview_inputs.portable_runtime', side_effect=fixture_extract):
            self.assertEqual(restore(self.consumer, self.identity), self.consumer / NATIVE)
        self.assertEqual(cached.read_bytes(), b'package probe')
        self.assertEqual((self.consumer / FRONTEND / 'assets/runtime.js').read_bytes(), b'asset')
        self.assertEqual((self.consumer / NATIVE / 'libmpv-2.dll').read_bytes(), b'portable bytes')


if __name__ == '__main__':
    unittest.main()
