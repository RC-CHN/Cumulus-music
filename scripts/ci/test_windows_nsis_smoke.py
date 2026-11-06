"""Synthetic provenance/layout checks for the opt-in actual NSIS diagnostic."""
import hashlib
import json
from pathlib import Path, PureWindowsPath
import tempfile
import unittest

from windows_nsis_smoke import (
    PLATFORM, clean_environment, existing_webview_version, installed_media_matches, nsis_command,
    require_scratch_registration, verified_installer,
)


class WindowsNsisProof(unittest.TestCase):
    def test_installer_requires_exact_source_attempt_and_hash(self):
        with tempfile.TemporaryDirectory() as folder:
            packages = Path(folder)
            commit = 'a' * 40
            version = '1.2.3-rc.1'
            name = f'Cumulus-{version}-{PLATFORM}-setup.exe'
            setup = packages / name
            setup.write_bytes(b'synthetic')
            asset = dict(name=name, bytes=setup.stat().st_size,
                         sha256=hashlib.sha256(setup.read_bytes()).hexdigest())
            (packages / f'manifest-{PLATFORM}.json').write_text(json.dumps(dict(
                version=version, commit=commit, platform=PLATFORM,
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


    def test_scratch_registry_requires_exact_current_user_install_and_uninstaller(self):
        directory = PureWindowsPath(r'C:\runner\scratch directory\installed application')
        version = '0.2.0-rc.1'
        row = dict(Key='Cumulus', DisplayName='Cumulus', DisplayVersion=version,
                   InstallLocation=f'"{directory}"', UninstallString=f'"{directory / "uninstall.exe"}"')
        require_scratch_registration([row], directory, version)
        for changed in (
            row | {'InstallLocation': r'"C:\Users\fixture\AppData\Local\Cumulus"'},
            row | {'UninstallString': row['UninstallString'] + ' /unexpected'},
            row | {'DisplayVersion': '0.2.0'},
        ):
            with self.assertRaisesRegex(AssertionError, 'exact scratch'):
                require_scratch_registration([changed], directory, version)
        with self.assertRaisesRegex(AssertionError, 'ambiguous'):
            require_scratch_registration([row, row], directory, version)

    def test_webview_runtime_preflight_refuses_bootstrap_or_wrong_registry_data(self):
        self.assertEqual(existing_webview_version('143.0.4.2', None), '143.0.4.2')
        self.assertEqual(existing_webview_version('', '143.0.4.2'), '143.0.4.2')
        for machine, user in ((None, None), ('0.0.0.0', '143.0.4.2'),
                              ('not-a-version', None), (None, '143.0')):
            with self.subTest(machine=machine, user=user):
                with self.assertRaisesRegex(AssertionError, 'not preinstalled'):
                    existing_webview_version(machine, user)

    def test_installer_children_do_not_inherit_browser_overrides_or_credential_environment(self):
        with tempfile.TemporaryDirectory() as folder:
            system = Path(folder)
            temp = system / 'scratch'
            inherited = {'SystemRoot': str(system), 'pAtH': 'ambient-dll-path', 'gH_ToKeN': 'token',
                         'CUMULUS_SESSION_DIR': 'unrelated-state', 'WEBVIEW2_USER_DATA_FOLDER': 'browser',
                         'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'runtime-token', 'tEmp': 'ambient-temp'}
            env = clean_environment(inherited, temp)
            self.assertEqual(set(env), {'SystemRoot', 'PATH', 'TEMP', 'TMP', 'PYTHONUTF8'})
            self.assertEqual(env['TEMP'], str(temp))
            self.assertEqual(env['TMP'], str(temp))
            self.assertNotIn('ambient', env['PATH'])


if __name__ == '__main__':
    unittest.main()
