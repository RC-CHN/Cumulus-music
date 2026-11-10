"""Same-attempt, fixed-inventory Windows package probe handoff."""
import json
from pathlib import Path
import re
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from windows_package_probes import (
    ARTIFACT, GROUPS, MANIFEST, MEDIA_PROOF, RUNTIME, RUNTIME_ARTIFACT,
    build, executable_receipt, stage, verify,
)


def synthetic_pe(label, *, machine=0x8664, flags=2):
    dos = bytearray(64)
    dos[:2] = b'MZ'
    struct.pack_into('<I', dos, 60, 64)
    return dos + struct.pack('<4sHHIIIHH', b'PE\0\0', machine, 0, 0, 0, 0, 0, flags) + label


class WindowsPackageProbes(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.git('init', '-q')
        (self.repo / '.gitignore').write_text('.build/\n', encoding='utf-8')
        self.git('add', '.gitignore')
        self.git('-c', 'user.name=Cumulus Fixture', '-c', 'user.email=fixture@example.test',
                 '-c', 'commit.gpgsign=false', 'commit', '-qm', 'fixture')
        self.identity = dict(commit=self.git('rev-parse', 'HEAD').strip(), ciRun='2345', ciAttempt='2')
        self.examples = self.repo / '.build/target/release/examples'
        self.examples.mkdir(parents=True)
        self.artifact = self.repo / ARTIFACT
        self.proof = self.repo / RUNTIME_ARTIFACT / RUNTIME / MEDIA_PROOF
        self.proof.parent.mkdir(parents=True)
        for names in GROUPS.values():
            for name in names:
                (self.examples / name).write_bytes(synthetic_pe(name.encode()))
        player = executable_receipt(self.examples / 'player_probe.exe')
        self.proof.write_text(json.dumps(dict(
            filterLoaded=True, nativePcmVerified=True, playerProbeSha256=player['sha256']
        )), encoding='utf-8')

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.repo, text=True)

    def stage_all(self):
        for group in GROUPS:
            stage(self.repo, self.identity, group, self.examples)

    def test_roundtrip_binds_each_executable_and_the_existing_player_smoke(self):
        self.stage_all()
        probes = verify(self.artifact, self.identity, self.proof)
        self.assertEqual(set(probes), {name for names in GROUPS.values() for name in names})
        for name, (file, receipt) in probes.items():
            self.assertEqual(file.read_bytes(), (self.examples / name).read_bytes())
            self.assertEqual(receipt, executable_receipt(self.examples / name))

    def test_wrong_commit_run_or_attempt_is_never_accepted(self):
        self.stage_all()
        for key, mismatch in (('commit', 'e' * 40), ('ciRun', '4'), ('ciAttempt', '1')):
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, 'provenance'):
                verify(self.artifact, self.identity | {key: mismatch}, self.proof)

    def test_missing_extra_truncated_and_same_length_changes_fail_closed(self):
        self.stage_all()
        path = self.artifact / 'helpers' / 'engine_roundtrip.exe'
        original = path.read_bytes()
        for altered in (original[:-1], original[:-1] + b'?'):
            path.write_bytes(altered)
            with self.assertRaisesRegex(AssertionError, 'checksum'):
                verify(self.artifact, self.identity, self.proof)
        path.unlink()
        with self.assertRaisesRegex(AssertionError, 'contents'):
            verify(self.artifact, self.identity, self.proof)
        path.write_bytes(original)
        (path.parent / 'unexpected.exe').write_bytes(original)
        with self.assertRaisesRegex(AssertionError, 'contents'):
            verify(self.artifact, self.identity, self.proof)

    def test_recomputed_player_manifest_cannot_bypass_the_verified_runtime_receipt(self):
        self.stage_all()
        path = self.artifact / 'verified-player' / 'player_probe.exe'
        path.write_bytes(synthetic_pe(b'a new binary with a different digest'))
        manifest = path.with_name(MANIFEST)
        contents = json.loads(manifest.read_text(encoding='utf-8'))
        contents['files'][path.name] = executable_receipt(path)
        manifest.write_text(json.dumps(contents), encoding='utf-8')
        with self.assertRaisesRegex(AssertionError, 'runtime-verified executable'):
            verify(self.artifact, self.identity, self.proof)

    def test_staging_requires_an_exact_clean_checkout_and_windows_x64_executables(self):
        with self.assertRaisesRegex(AssertionError, 'checkout differs'):
            stage(self.repo, self.identity | {'commit': 'e' * 40}, 'helpers', self.examples)
        (self.repo / 'untracked').touch()
        with self.assertRaisesRegex(AssertionError, 'checkout changed'):
            stage(self.repo, self.identity, 'helpers', self.examples)
        (self.repo / 'untracked').unlink()
        path = self.examples / 'engine_roundtrip.exe'
        for payload in (b'not PE', synthetic_pe(b'x86', machine=0x14c), synthetic_pe(b'DLL', flags=0x2002)):
            path.write_bytes(payload)
            with self.assertRaisesRegex(AssertionError, 'executable'):
                stage(self.repo, self.identity, 'helpers', self.examples)
        self.assertFalse(self.artifact.exists())

    def test_no_symlink_is_followed_when_verifying_a_probe(self):
        self.stage_all()
        path = self.artifact / 'helpers' / 'engine_roundtrip.exe'
        path.unlink()
        try:
            path.symlink_to(self.examples / path.name)
        except (OSError, NotImplementedError) as error:
            self.skipTest(str(error))
        with self.assertRaisesRegex(AssertionError, 'linked'):
            verify(self.artifact, self.identity, self.proof)

    def test_parallel_build_never_relinks_the_verified_player(self):
        with patch('windows_package_probes.subprocess.run') as run:
            build(self.repo)
        command = run.call_args.args[0]
        self.assertTrue({'--release', '--locked'} <= set(command))
        self.assertEqual({command[i + 1] for i, item in enumerate(command) if item == '--example'},
                         {'engine_roundtrip', 'parametric_probe', 'pinned_audio_probe'})
        self.assertNotIn('cumulus-desktop', command)

    def test_workflow_requires_both_producers_and_keeps_package_proofs_after_the_exact_zip(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/ci.yml').read_text(encoding='utf-8')

        def job(name):
            return re.search(r'^  ' + re.escape(name) + r':\n((?:^    .*\n|^\n)*)', workflow, re.M)[1]

        self.assertNotIn('needs:', job('windows-package-probes'))
        self.assertIn('windows_package_probes.py helpers', job('windows-package-probes'))
        self.assertIn('windows_package_probes.py verified-player', job('windows-release-runtime'))
        package = job('package-windows')
        self.assertIn('needs: [frontend, windows-release-runtime, windows-package-probes]', package)
        self.assertIn('--windows-probes .build/windows-package-probes', package)
        for group in GROUPS:
            self.assertIn(f'windows-package-probes-{group}-attempt-${{{{ github.run_attempt }}}}', package)
        self.assertIn('needs: [package-windows, windows-webview-probes]', job('windows-webview-packaged'))
        self.assertIn('test "$WINDOWS_PACKAGE_PROBES" = success', job('gate'))


if __name__ == '__main__':
    unittest.main()
