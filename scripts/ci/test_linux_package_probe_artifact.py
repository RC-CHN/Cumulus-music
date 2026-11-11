import json
import os
from pathlib import Path
import stat
import tempfile
import unittest

from linux_package_probe_artifact import (
    CATALOG_SHARDS, CORE, LOCAL_SHARDS, RECEIPT, SHARDS, restore, stage,
)


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
        self.write_probes(SHARDS)
        stage(self.artifact, self.examples, self.examples / CORE, self.dist, self.identity)

    def write_probes(self, shards):
        header = b'\x7fELF\x02\x01\x01' + bytes(9) + b'\x03\x00\x3e\x00' + bytes(44)
        for name in {name for names in shards.values() for name in names}:
            path = self.examples / name
            path.write_bytes(header + name.encode())
            path.chmod(0o755)

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

    def test_local_desktop_shards_share_one_probe_without_a_core_harness(self):
        suite = 'local-desktop'
        self.write_probes(LOCAL_SHARDS)
        local = self.artifact.parent / 'local'
        stage(local, self.examples, None, self.dist, self.identity, suite=suite)
        mini_digests = set()
        for shard, names in LOCAL_SHARDS.items():
            source = local / shard
            output = self.output / shard
            with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
                restore(source, output, shard, self.dist, self.identity | {'ciAttempt': '3'}, suite=suite)
            self.assertFalse(any(output.iterdir()))
            result = json.loads(restore(source, output, shard, self.dist, self.identity, suite=suite))
            self.assertEqual(result['kind'], 'linux-local-desktop-probes-v1')
            self.assertEqual({file.name for file in output.iterdir()}, set(names))
            mini_digests.add(next(row['sha256'] for row in result['binaries'] if row['name'] == 'mini_ui_probe'))
        self.assertEqual(len(mini_digests), 1)

    def test_catalog_shards_reuse_one_binary_and_reject_a_replaced_probe(self):
        suite = 'catalog'
        self.write_probes(CATALOG_SHARDS)
        catalog = self.artifact.parent / 'catalog'
        stage(catalog, self.examples, None, self.dist, self.identity, suite=suite)
        digests = set()
        for shard, names in CATALOG_SHARDS.items():
            source = catalog / shard
            output = self.output / shard
            result = json.loads(restore(source, output, shard, self.dist, self.identity, suite=suite))
            self.assertEqual(result['kind'], 'linux-catalog-probes-v1')
            self.assertEqual({file.name for file in output.iterdir()}, set(names))
            digests.add(next(row['sha256'] for row in result['binaries']
                             if row['name'] == 'catalog_ui_probe'))
        self.assertEqual(len(digests), 1)
        source = catalog / 'catalog-community'
        probe = source / 'comment_edit_ui_probe'
        probe.write_bytes(probe.read_bytes() + b'changed')
        replacement = self.output / 'replacement'
        with self.assertRaisesRegex(ValueError, 'receipt mismatch'):
            restore(source, replacement, 'catalog-community', self.dist, self.identity, suite=suite)
        self.assertFalse(any(replacement.iterdir()))


if __name__ == '__main__':
    unittest.main()
