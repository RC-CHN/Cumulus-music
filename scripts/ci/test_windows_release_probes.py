"""Release examples may move between CI jobs, but not between runs or commits."""
from pathlib import Path
import re
import tempfile
import unittest

from windows_release_probes import BINARIES, MANIFEST, stage, verify
from windows_webview_inputs import ci_identity


class WindowsReleaseProbes(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.examples, self.artifact = root / 'examples', root / 'artifact'
        self.examples.mkdir()
        self.identity = ci_identity(dict(GITHUB_SHA='a' * 40, GITHUB_RUN_ID='123', GITHUB_RUN_ATTEMPT='2'))
        for name in BINARIES:
            (self.examples / name).write_bytes(name.encode())
        stage(self.examples, self.artifact, self.identity)

    def test_only_exact_same_attempt_binaries_are_accepted(self):
        self.assertEqual({p.name for p in self.artifact.iterdir()}, {MANIFEST, *BINARIES})
        verify(self.artifact, self.identity)
        for name in BINARIES:
            self.assertEqual((self.artifact / name).read_bytes(), (self.examples / name).read_bytes())
        for change in ({'commit': 'b' * 40}, {'ciRun': '234'}, {'ciAttempt': '1'}):
            with self.subTest(change=change), self.assertRaisesRegex(AssertionError, 'provenance'):
                verify(self.artifact, self.identity | change)

    def test_rejects_tampered_missing_or_extra_executables(self):
        probe = self.artifact / BINARIES[0]
        data = probe.read_bytes()
        probe.write_bytes(b'!' + data[1:])
        with self.assertRaisesRegex(AssertionError, 'checksum'):
            verify(self.artifact, self.identity)
        probe.unlink()
        with self.assertRaisesRegex(AssertionError, 'contents'):
            verify(self.artifact, self.identity)
        probe.write_bytes(data)
        (self.artifact / 'unexpected.exe').write_bytes(data)
        with self.assertRaisesRegex(AssertionError, 'contents'):
            verify(self.artifact, self.identity)

    def test_workflow_starts_probe_build_early_and_requires_exact_artifact(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/ci.yml').read_text()

        def job(name):
            body = workflow.split(f'  {name}:\n', 1)[1]
            return re.split(r'^  [\w-]+:\s*$', body, maxsplit=1, flags=re.MULTILINE)[0]

        producer = job('windows-release-probes')
        package = job('package-windows')
        gate = job('gate')
        self.assertNotIn('needs:', producer)
        self.assertIn('python scripts/ci/windows_release_probes.py', producer)
        artifact = 'name: windows-release-probes-attempt-${{ github.run_attempt }}'
        self.assertIn(artifact, producer)
        self.assertIn(artifact, package)
        self.assertIn('needs: [frontend, windows-release-runtime, windows-release-probes]', package)
        self.assertIn('--windows-runtime .build/windows-release-runtime', package)
        self.assertIn('--windows-probes .build/windows-release-probes', package)
        self.assertIn('WINDOWS_RELEASE_PROBES: ${{ needs.windows-release-probes.result }}', gate)
        self.assertIn('test "$WINDOWS_RELEASE_PROBES" = success', gate)
        self.assertIn('needs: [package-windows, windows-webview-probes]', job('windows-webview-packaged'))


if __name__ == '__main__':
    unittest.main()
