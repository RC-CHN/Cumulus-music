"""Portable ZIP proof rejects misplaced or altered runtime files before launch."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

import windows_flac as flac
from windows_zip_smoke import PLATFORM, REQUIRED, portable_runtime


class WindowsZipProof(unittest.TestCase):
    def check_decoder(self, *, fault=None):
        with tempfile.TemporaryDirectory() as folder, patch.dict('os.environ', {}, clear=True):
            root = Path(folder)
            stage = root / 'staged'
            stage.mkdir()
            payloads = {name: name.encode() for name in {flac.FLAC, flac.FLAC_LICENSE, *flac.FLAC_IMPORTS}}
            for name, data in payloads.items():
                path = stage / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            expected = dict(FLAC_DLL_BYTES=len(payloads[flac.FLAC]),
                            FLAC_DLL_SHA256=flac.sha256(stage / flac.FLAC),
                            FLAC_LICENSE_BYTES=len(payloads[flac.FLAC_LICENSE]),
                            FLAC_LICENSE_SHA256=flac.sha256(stage / flac.FLAC_LICENSE))
            version, commit = '1.2.3', 'a' * 40
            top = f'Cumulus-{version}-{PLATFORM}'
            archive = root / f'{top}.zip'
            with patch.multiple(flac, **expected):
                receipt = flac.decoder_receipt(stage, flac.PE_IMPORTS)
                runtime = dict(version=version, commit=commit, platform=PLATFORM, flacDecoder=receipt,
                               dlls=sorted({'libmpv-2.dll', flac.FLAC.lower(), *flac.FLAC_IMPORTS}))
                if fault == 'no-license':
                    del payloads[flac.FLAC_LICENSE]
                if fault == 'changed-dependency':
                    payloads['libogg-0.dll'] = b'x' + payloads['libogg-0.dll'][1:]
                if fault == 'no-receipt':
                    del runtime['flacDecoder']
                with zipfile.ZipFile(archive, 'w') as package:
                    package.writestr(f'{top}/runtime-manifest.json', json.dumps(runtime))
                    for name in REQUIRED:
                        package.writestr(f'{top}/{name}', name)
                    for name, data in payloads.items():
                        package.writestr(f'{top}/{name}', data)
                manifest = dict(version=version, commit=commit, platform=PLATFORM, flacDecoder=receipt,
                                assets=[dict(name=archive.name, bytes=archive.stat().st_size, sha256=flac.sha256(archive))])
                (root / f'manifest-{PLATFORM}.json').write_text(json.dumps(manifest), encoding='utf-8')
                portable = portable_runtime(root, commit, root, require_package_flac=True)
                self.assertEqual((portable / flac.FLAC).read_bytes(), payloads[flac.FLAC])
                self.assertEqual((portable / 'libogg-0.dll').read_bytes(), payloads['libogg-0.dll'])

    def test_installed_decoder_and_license_match_both_manifests(self):
        self.check_decoder()

    def test_rejects_package_without_pinned_decoder_notice(self):
        with self.assertRaisesRegex(AssertionError, 'missing pinned FLAC or its notice'):
            self.check_decoder(fault='no-license')

    def test_rejects_dependency_changes_inside_an_authenticated_zip(self):
        with self.assertRaisesRegex(AssertionError, 'checksum: libogg-0.dll'):
            self.check_decoder(fault='changed-dependency')

    def test_rejects_zip_that_omits_the_external_decoder_receipt(self):
        with self.assertRaisesRegex(AssertionError, 'missing or mismatched Windows FLAC manifest'):
            self.check_decoder(fault='no-receipt')

    def exercise(self, *, version='1.2.3', misplaced=False, changed=False, ci_identity=None):
        with tempfile.TemporaryDirectory() as folder, patch.dict('os.environ', {}, clear=True):
            root = Path(folder)
            commit = 'a' * 40
            top = f'Cumulus-{version}-{PLATFORM}'
            archive = root / f'{top}.zip'
            runtime = dict(version=version, commit=commit, platform=PLATFORM, dlls=['libmpv-2.dll'])
            with zipfile.ZipFile(archive, 'w') as package:
                package.writestr(f'{top}/runtime-manifest.json', json.dumps(runtime))
                for name in REQUIRED:
                    prefix = f'{top}/nested' if misplaced and name == 'cumulus.exe' else top
                    package.writestr(f'{prefix}/{name}', name)
                package.writestr(f'{top}/third-party/licenses/example', 'not executable')
            asset = dict(name=archive.name, bytes=archive.stat().st_size,
                         sha256=hashlib.sha256(archive.read_bytes()).hexdigest())
            manifest = dict(version=version, commit=commit, platform=PLATFORM,
                            ciRun='17', ciAttempt='2', assets=[asset])
            (root / f'manifest-{PLATFORM}.json').write_text(json.dumps(manifest), encoding='utf-8')
            if changed:
                content = archive.read_bytes()
                archive.write_bytes(bytes([content[0] ^ 1]) + content[1:])
            portable = portable_runtime(root, commit, root, **(ci_identity or {}))
            self.assertEqual({file.name for file in portable.iterdir()}, REQUIRED)
            self.assertEqual((portable / 'cumulus.exe').read_bytes(), b'cumulus.exe')

    def test_preserves_exact_runtime_content_and_adjacency(self):
        for version in ('1.2.3', '1.2.3-rc.1'):
            with self.subTest(version=version):
                self.exercise(version=version)

    def test_rejects_valid_archive_with_misplaced_executable(self):
        with self.assertRaisesRegex(AssertionError, 'missing adjacent runtime files'):
            self.exercise(misplaced=True)

    def test_rejects_same_size_changed_archive(self):
        with self.assertRaisesRegex(AssertionError, 'checksum mismatch'):
            self.exercise(changed=True)


    def test_rejects_same_zip_from_another_ci_run_or_attempt(self):
        self.exercise(ci_identity=dict(ci_run='17', ci_attempt='2'))
        for field, identity in (('ciRun', dict(ci_run='18', ci_attempt='2')),
                                ('ciAttempt', dict(ci_run='17', ci_attempt='1'))):
            with self.subTest(field=field), self.assertRaisesRegex(AssertionError, field):
                self.exercise(ci_identity=identity)
