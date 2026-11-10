"""Reject the wrong run, changed bytes, and binaries outside the release-probe contract."""

import json
from pathlib import Path
import struct
import tempfile
import unittest

from windows_package_probes import EXECUTABLES, MANIFEST, RELEASE_PROBE_BUILD, stage, verify


def synthetic_pe(name, *, machine=0x8664, flags=0x0002):
    header = bytearray(90)
    header[:2] = b'MZ'
    struct.pack_into('<I', header, 60, 64)
    header[64:68] = b'PE\0\0'
    struct.pack_into('<H', header, 68, machine)
    struct.pack_into('<HH', header, 86, flags, 0x20B)
    return bytes(header) + name.encode()


class WindowsPackageProbes(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.examples = root / 'examples'
        self.examples.mkdir()
        self.artifact = root / 'artifact'
        self.identity = dict(commit='a' * 40, ciRun='123', ciAttempt='2')
        for name in EXECUTABLES:
            (self.examples / name).write_bytes(synthetic_pe(name))

    def test_exact_locked_release_build_and_probe_inventory(self):
        self.assertEqual(RELEASE_PROBE_BUILD, (
            'cargo', 'build', '--release', '--locked', '-p', 'cumulus-core', '-p', 'cumulus-audio-filter', '--lib',
            '--example', 'engine_roundtrip', '--example', 'player_probe', '--example', 'pinned_audio_probe',
            '--example', 'parametric_probe',
        ))
        stage(self.examples, self.artifact, self.identity)
        verify(self.artifact, self.identity)
        manifest = json.loads((self.artifact / MANIFEST).read_text(encoding='utf-8'))
        self.assertEqual(set(manifest['binaries']), set(EXECUTABLES))
        for name in EXECUTABLES:
            self.assertEqual((self.examples / name).read_bytes(), (self.artifact / name).read_bytes())

    def test_mixed_commit_run_attempt_and_same_size_change_are_rejected(self):
        stage(self.examples, self.artifact, self.identity)
        for key, wrong in (('commit', 'b' * 40), ('ciRun', '124'), ('ciAttempt', '1')):
            with self.subTest(key=key), self.assertRaisesRegex(AssertionError, 'provenance'):
                verify(self.artifact, self.identity | {key: wrong})
        changed = self.artifact / EXECUTABLES[0]
        changed.write_bytes(changed.read_bytes()[:-1] + b'!')
        with self.assertRaisesRegex(AssertionError, 'checksum'):
            verify(self.artifact, self.identity)

    def test_final_application_and_missing_probe_are_rejected(self):
        stage(self.examples, self.artifact, self.identity)
        unexpected = self.artifact / 'cumulus.exe'
        unexpected.write_bytes(synthetic_pe(unexpected.name))
        with self.assertRaisesRegex(AssertionError, 'unexpected .* contents'):
            verify(self.artifact, self.identity)
        unexpected.unlink()
        (self.artifact / EXECUTABLES[0]).unlink()
        with self.assertRaisesRegex(AssertionError, 'unexpected .* contents'):
            verify(self.artifact, self.identity)

    def test_wrong_architecture_and_dll_cannot_be_staged(self):
        probe = self.examples / EXECUTABLES[0]
        for kwargs, error in (({'machine': 0x14C}, 'x64'), ({'flags': 0x2002}, 'not a DLL')):
            with self.subTest(kwargs=kwargs):
                probe.write_bytes(synthetic_pe(probe.name, **kwargs))
                with self.assertRaisesRegex(AssertionError, error):
                    stage(self.examples, self.artifact, self.identity)
                self.assertFalse(self.artifact.exists())


if __name__ == '__main__':
    unittest.main()
