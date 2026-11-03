"""Synthetic provenance/layout checks for the opt-in actual NSIS diagnostic."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from windows_nsis_smoke import PLATFORM, installed_media_matches, nsis_command, verified_installer


class WindowsNsisProof(unittest.TestCase):
    def test_installer_requires_exact_source_attempt_and_hash(self):
        with tempfile.TemporaryDirectory() as folder:
            packages = Path(folder)
            commit = 'a' * 40
            name = f'Cumulus-1.2.3-{PLATFORM}-setup.exe'
            setup = packages / name
            setup.write_bytes(b'synthetic')
            asset = dict(name=name, bytes=setup.stat().st_size,
                         sha256=hashlib.sha256(setup.read_bytes()).hexdigest())
            (packages / f'manifest-{PLATFORM}.json').write_text(json.dumps(dict(
                version='1.2.3', commit=commit, platform=PLATFORM,
                ciRun='37', ciAttempt='2', assets=[asset],
            )), encoding='utf-8')
            self.assertEqual(verified_installer(packages, commit, '37', '2'), setup)
            with self.assertRaisesRegex(AssertionError, 'wrong installer provenance'):
                verified_installer(packages, commit, '37', '1')
            setup.write_bytes(b'different')
            with self.assertRaisesRegex(AssertionError, 'checksum mismatch'):
                verified_installer(packages, commit, '37', '2')

    def test_installed_runtime_needs_adjacent_exact_files(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            portable, installed = root / 'zip', root / 'actual installation'
            for directory in (portable, installed):
                directory.mkdir()
                (directory / 'libmpv-2.dll').write_bytes(b'expected media')
                (directory / 'cumulus.exe').write_bytes(b'expected application')
            self.assertEqual(installed_media_matches(installed, portable), 2)
            (installed / 'cumulus.exe').write_bytes(b'altered! application')
            with self.assertRaisesRegex(AssertionError, 'differs'):
                installed_media_matches(installed, portable)
            nested = installed / 'native'
            nested.mkdir()
            (installed / 'libmpv-2.dll').rename(nested / 'libmpv-2.dll')
            with self.assertRaisesRegex(AssertionError, 'DLL closure'):
                installed_media_matches(installed, portable)

    def test_destination_directives_keep_raw_final_path_with_spaces(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder).resolve()
            destination = root / 'actual installation'
            setup = root / 'bundled installer.exe'
            self.assertTrue(nsis_command(setup, destination).endswith(f' /S /D={destination}'))
            self.assertTrue(nsis_command(setup, destination, remove=True).endswith(f' /S _?={destination}'))


if __name__ == '__main__':
    unittest.main()
