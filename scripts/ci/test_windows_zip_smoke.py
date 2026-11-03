"""Portable ZIP proof rejects misplaced or altered runtime files before launch."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from windows_zip_smoke import PLATFORM, REQUIRED, portable_runtime


class WindowsZipProof(unittest.TestCase):
    def exercise(self, *, misplaced=False, changed=False):
        with tempfile.TemporaryDirectory() as folder, patch.dict('os.environ', {}, clear=True):
            root = Path(folder)
            version, commit = '1.2.3', 'a' * 40
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
            manifest = dict(version=version, commit=commit, platform=PLATFORM, assets=[asset])
            (root / f'manifest-{PLATFORM}.json').write_text(json.dumps(manifest), encoding='utf-8')
            if changed:
                content = archive.read_bytes()
                archive.write_bytes(bytes([content[0] ^ 1]) + content[1:])
            portable = portable_runtime(root, commit, root)
            self.assertEqual({file.name for file in portable.iterdir()}, REQUIRED)
            self.assertEqual((portable / 'cumulus.exe').read_bytes(), b'cumulus.exe')

    def test_preserves_exact_runtime_content_and_adjacency(self):
        self.exercise()

    def test_rejects_valid_archive_with_misplaced_executable(self):
        with self.assertRaisesRegex(AssertionError, 'missing adjacent runtime files'):
            self.exercise(misplaced=True)

    def test_rejects_same_size_changed_archive(self):
        with self.assertRaisesRegex(AssertionError, 'checksum mismatch'):
            self.exercise(changed=True)
