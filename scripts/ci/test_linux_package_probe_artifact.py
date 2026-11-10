import json
import os
from pathlib import Path
import stat
import tempfile
import unittest

from linux_package_probe_artifact import CORE, RECEIPT, SHARDS, restore, stage


@unittest.skipUnless(os.name == 'posix', 'Linux executable mode and links')
class LinuxPackageProbeArtifact(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.dist = root / 'dist'
        self.dist.mkdir()
        self.entry = self.dist / 'index.html'
        self.entry.write_text('<title>synthetic frontend</title>')
        self.examples = root / 'built'
        self.examples.mkdir()
        self.artifact = root / 'artifact'
        self.output = root / 'installed'
        self.identity = {'commit': 'a' * 40, 'ciRun': '17', 'ciAttempt': '2'}
        header = b'\x7fELF\x02\x01\x01' + bytes(9) + b'\x03\x00\x3e\x00' + bytes(44)
        for name in {name for names in SHARDS.values() for name in names}:
            path = self.examples / name
            path.write_bytes(header + name.encode())
            path.chmod(0o755)
        stage(self.artifact, self.examples, self.examples / CORE, self.dist, self.identity)

    def test_fixed_shards_restore_exact_executables_with_the_same_harness(self):
        core_digests = set()
        for shard, names in SHARDS.items():
            source = self.artifact / shard
            result = json.loads(restore(source, self.output / shard, shard, self.dist, self.identity))
            self.assertEqual(result['shard'], shard)
            core_digests.add(next(row['sha256'] for row in result['binaries'] if row['name'] == CORE))
            self.assertEqual({file.name for file in (self.output / shard).iterdir()}, set(names))
            for name in names:
                installed = self.output / shard / name
                self.assertEqual(installed.read_bytes(), (self.examples / name).read_bytes())
                self.assertEqual(stat.S_IMODE(installed.stat().st_mode), 0o755)
        self.assertEqual(len(core_digests), 1)

    def test_another_attempt_dist_or_changed_binary_installs_nothing(self):
        shard = 'media-startup'
        source = self.artifact / shard
        for key, value in (('commit', 'b' * 40), ('ciRun', '18'), ('ciAttempt', '1')):
            with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
                restore(source, self.output, shard, self.dist, self.identity | {key: value})
        old_entry = self.entry.read_bytes()
        self.entry.write_text('<title>changed frontend</title>')
        with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
            restore(source, self.output, shard, self.dist, self.identity)
        self.entry.write_bytes(old_entry)
        binary = source / 'loudness_ui_probe'
        binary.write_bytes(binary.read_bytes() + b'changed')
        with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
            restore(source, self.output, shard, self.dist, self.identity)
        self.assertFalse(any(self.output.iterdir()))

    def test_wrong_shard_or_symlink_cannot_install(self):
        source = self.artifact / 'media-decoder'
        with self.assertRaisesRegex(ValueError, 'artifact contents'):
            restore(source, self.output, 'media-startup', self.dist, self.identity)
        binary = source / CORE
        binary.unlink()
        binary.symlink_to(self.examples / CORE)
        with self.assertRaisesRegex(ValueError, 'artifact file'):
            restore(source, self.output, 'media-decoder', self.dist, self.identity)
        self.assertFalse(self.output.exists())

    def test_existing_destination_is_neither_overwritten_nor_left_partially_installed(self):
        self.output.mkdir()
        existing = self.output / 'loudness_ui_probe'
        existing.write_bytes(b'preexisting')
        with self.assertRaises(FileExistsError):
            restore(self.artifact / 'media-startup', self.output, 'media-startup', self.dist, self.identity)
        self.assertEqual(list(self.output.iterdir()), [existing])
        self.assertEqual(existing.read_bytes(), b'preexisting')
        self.assertTrue((self.artifact / 'media-startup' / RECEIPT).is_file())


if __name__ == '__main__':
    unittest.main()
