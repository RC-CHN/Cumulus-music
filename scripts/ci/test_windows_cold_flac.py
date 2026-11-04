import hashlib
import json
from pathlib import Path
from subprocess import CompletedProcess
import tempfile
import unittest
from unittest.mock import patch

import windows_cold_flac as proof


class WindowsColdFlacTests(unittest.TestCase):
    def test_rejects_an_unverified_dll_already_in_the_package(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(proof, 'download') as download:
            root = Path(directory)
            target = root / proof.FLAC
            target.write_bytes(b'incidental DLL')
            with self.assertRaisesRegex(ValueError, 'package-stage libFLAC'):
                proof.ensure_flac(root, root, root)
            download.assert_not_called()
            self.assertEqual(target.read_bytes(), b'incidental DLL')

    def test_reuses_installed_ucrt_only_when_the_binary_hash_matches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / 'runtime'
            runtime.mkdir()
            installed = root / 'ucrt64/bin' / proof.FLAC
            installed.parent.mkdir(parents=True)
            installed.write_bytes(b'audited binary')
            digest = hashlib.sha256(installed.read_bytes()).hexdigest()
            with patch.object(proof, 'FLAC_DLL_SHA256', digest), patch.object(proof, 'download') as download:
                self.assertEqual(proof.ensure_flac(runtime, root, root), 'msys2-ucrt64-installed')
                self.assertEqual((runtime / proof.FLAC).read_bytes(), installed.read_bytes())
                download.assert_not_called()

    def test_mismatched_installed_dll_extracts_only_the_locked_archive_member(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runtime = root / 'runtime'
            runtime.mkdir()
            installed = root / 'ucrt64/bin' / proof.FLAC
            installed.parent.mkdir(parents=True)
            installed.write_bytes(b'not the audited DLL')
            pinned = b'audited binary'
            digest = hashlib.sha256(pinned).hexdigest()
            with patch.object(proof, 'FLAC_DLL_SHA256', digest), patch.object(proof, 'download') as download, \
                    patch.object(proof.subprocess, 'check_output', side_effect=['/proof/package\n', pinned]) as execute:
                self.assertEqual(proof.ensure_flac(runtime, root, root), 'pinned-msys2-archive')
            download.assert_called_once_with(proof.FLAC_URL, root / proof.FLAC_ARCHIVE, proof.FLAC_ARCHIVE_SHA256)
            self.assertEqual(execute.call_args.args[0][1:], ['-xOf', '/proof/package', f'ucrt64/bin/{proof.FLAC}'])
            self.assertEqual((runtime / proof.FLAC).read_bytes(), pinned)
            self.assertEqual(installed.read_bytes(), b'not the audited DLL')

    def test_requires_exact_test_result_and_native_identity_receipt(self):
        name = proof.IDENTITY
        missing = CompletedProcess([], 0, 'test result: ok. 0 passed; 0 failed; 0 ignored')
        with self.assertRaisesRegex(RuntimeError, 'did not pass exactly once'):
            proof.case_result(missing, name)
        success = f'test {name} ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored'
        with self.assertRaisesRegex(ValueError, 'identity receipt is missing'):
            proof.case_result(CompletedProcess([], 0, success), name)
        identity = {'libflacVersion': '1.5.0', 'relativeLookupRejected': True}
        stdout = f'{proof.RECEIPT}{json.dumps(identity)}\n{success}'
        self.assertEqual(proof.case_result(CompletedProcess([], 0, stdout), name), identity)

    def test_report_rejects_a_loaded_dll_from_outside_the_runtime(self):
        with tempfile.TemporaryDirectory() as directory:
            runtime = Path(directory) / 'runtime'
            runtime.mkdir()
            flac, mpv = runtime / proof.FLAC, runtime / proof.MPV
            other = runtime.parent / proof.FLAC
            for path in (flac, mpv, other):
                path.write_bytes(b'identical contents, distinct file identities')
            receipt = {'libflacVersion': '1.5.0', 'libmpvApi': 0x20005, 'libmpvVersion': 'mpv 0.41.0',
                       'relativeLookupRejected': True, 'libflacModule': str(flac), 'libmpvModule': str(mpv)}
            proof.check_identity(receipt, runtime)
            receipt['libflacModule'] = str(other)
            with self.assertRaisesRegex(ValueError, 'does not identify'):
                proof.check_identity(receipt, runtime)


if __name__ == '__main__':
    unittest.main()
