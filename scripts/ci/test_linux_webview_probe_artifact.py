import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest

from linux_webview_probe_artifact import APT_CACHE, BINARY, RECEIPT, restore, run_identity, stage


@unittest.skipUnless(os.name == 'posix', 'Linux executable mode and links')
class LinuxWebViewProbeArtifact(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.dist = root / 'dist'
        (self.dist / 'assets').mkdir(parents=True)
        (self.dist / 'index.html').write_text('<script src="assets/app.js"></script>')
        self.asset = self.dist / 'assets' / 'app.js'
        self.asset.write_text('fixture')
        self.original = root / 'built' / BINARY
        self.original.parent.mkdir()
        header = b'\x7fELF\x02\x01\x01' + bytes(9) + b'\x03\x00\x3e\x00' + bytes(44)
        self.binary = header + b'fixture bytes'
        self.original.write_bytes(self.binary)
        self.original.chmod(0o755)
        self.installed = root / 'out' / BINARY
        self.artifact = root / 'artifact'
        self.identity = run_identity(
            {'GITHUB_SHA': 'a' * 40, 'GITHUB_RUN_ID': '78', 'GITHUB_RUN_ATTEMPT': '2'}, 'a' * 40,
        )
        stage(self.original, self.dist, self.artifact, self.identity)

    def test_two_consumers_restore_the_same_binary_with_executable_mode(self):
        payload = self.artifact / BINARY
        payload.chmod(0o644)
        second = self.installed.parent / 'second' / BINARY
        for target in (self.installed, second):
            result = json.loads(restore(self.artifact, self.dist, target, self.identity))
            self.assertEqual(result['binary']['sha256'], hashlib.sha256(self.binary).hexdigest())
            self.assertEqual(target.read_bytes(), self.binary)
            self.assertEqual(stat.S_IMODE(target.stat().st_mode), 0o755)

    def test_another_origin_or_asset_tree_cannot_restore_the_probe(self):
        for field, value in (('commit', 'b' * 40), ('ciRun', '79'), ('ciAttempt', '1')):
            with self.subTest(field=field):
                with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
                    restore(self.artifact, self.dist, self.installed, self.identity | {field: value})
                self.assertFalse(self.installed.exists())
        self.asset.write_text('changed')
        with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
            restore(self.artifact, self.dist, self.installed, self.identity)
        self.assertFalse(self.installed.exists())

    def test_wrong_checkout_or_noncanonical_ci_identity_is_rejected(self):
        valid = {'GITHUB_SHA': 'a' * 40, 'GITHUB_RUN_ID': '78', 'GITHUB_RUN_ATTEMPT': '2'}
        with self.assertRaisesRegex(ValueError, 'checkout/run/attempt'):
            run_identity(valid, 'b' * 40)
        for name, value in (('GITHUB_RUN_ID', '0'), ('GITHUB_RUN_ATTEMPT', '02'), ('GITHUB_SHA', 'A' * 40)):
            with self.subTest(name=name):
                with self.assertRaisesRegex(ValueError, 'checkout/run/attempt'):
                    run_identity(valid | {name: value}, 'a' * 40)

    def test_altered_binary_cannot_be_launched(self):
        payload = self.artifact / BINARY
        for damaged in (self.binary[:-1], self.binary[:-1] + b'!', b'not an executable'):
            with self.subTest(damaged=damaged[-14:]):
                payload.write_bytes(damaged)
                with self.assertRaises(ValueError):
                    restore(self.artifact, self.dist, self.installed, self.identity)
                self.assertFalse(self.installed.exists())

    def test_optional_bad_apt_cache_does_not_change_native_probe_authority(self):
        # The packages have their own signed admission. Even malformed
        # optional content cannot satisfy or invalidate the probe receipt.
        (self.artifact / APT_CACHE).write_bytes(b'invalid optional APT handoff')
        restore(self.artifact, self.dist, self.installed, self.identity)
        self.assertEqual(self.installed.read_bytes(), self.binary)
        self.installed.unlink()
        (self.artifact / BINARY).write_bytes(self.binary[:-1] + b'!')
        with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
            restore(self.artifact, self.dist, self.installed, self.identity)
        self.assertFalse(self.installed.exists())

    def test_incomplete_or_unexpected_artifact_does_not_restore(self):
        unexpected = self.artifact / 'other'
        unexpected.touch()
        with self.assertRaisesRegex(ValueError, 'contents'):
            restore(self.artifact, self.dist, self.installed, self.identity)
        unexpected.unlink()
        manifest = self.artifact / RECEIPT
        manifest.write_bytes(b'{broken')
        with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
            restore(self.artifact, self.dist, self.installed, self.identity)
        manifest.unlink()
        manifest.symlink_to(self.original)
        with self.assertRaisesRegex(ValueError, 'artifact file'):
            restore(self.artifact, self.dist, self.installed, self.identity)
        self.assertFalse(self.installed.exists())

    def test_existing_executable_is_not_overwritten(self):
        self.installed.parent.mkdir()
        self.installed.write_bytes(b'preexisting')
        with self.assertRaises(FileExistsError):
            restore(self.artifact, self.dist, self.installed, self.identity)
        self.assertEqual(self.installed.read_bytes(), b'preexisting')


if __name__ == '__main__':
    unittest.main()
