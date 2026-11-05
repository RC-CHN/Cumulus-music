import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import linux_flac150 as proof


class FlacBuildBoundaries(unittest.TestCase):
    def archive(self, path, entries):
        with tarfile.open(path, 'w:xz') as out:
            for name, kind, body in entries:
                item = tarfile.TarInfo(name)
                item.type = kind
                item.size = len(body) if kind == tarfile.REGTYPE else 0
                if kind == tarfile.SYMTYPE:
                    item.linkname = '../outside'
                out.addfile(item, io.BytesIO(body) if item.isfile() else None)

    def test_safe_sources_are_copied_without_installing_or_applying_archive_permissions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / 'source.tar.xz'
            self.archive(archive, [('flac-1.5.0/CMakeLists.txt', tarfile.REGTYPE, b'project(FLAC)'),
                                   ('flac-1.5.0/src/a.c', tarfile.REGTYPE, b'int source;')])
            source = proof.unpack(archive, root / 'source')
            self.assertEqual((source / 'src/a.c').read_bytes(), b'int source;')
            self.assertFalse((root / 'installed').exists())

    def test_link_traversal_foreign_root_and_duplicate_member_are_rejected(self):
        cases = [
            [('flac-1.5.0/link', tarfile.SYMTYPE, b'')],
            [('flac-1.5.0/../../outside', tarfile.REGTYPE, b'x')],
            [('/absolute', tarfile.REGTYPE, b'x')],
            [('another-project/CMakeLists.txt', tarfile.REGTYPE, b'x')],
            [('flac-1.5.0/a', tarfile.REGTYPE, b'x'), ('flac-1.5.0/a', tarfile.REGTYPE, b'x')],
        ]
        for entries in cases:
            with self.subTest(entries=entries), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                self.archive(root / 'source.tar.xz', entries)
                with self.assertRaisesRegex(ValueError, 'unsafe FLAC archive member'):
                    proof.unpack(root / 'source.tar.xz', root / 'source')

    def test_declared_file_and_total_source_bounds_fail_before_copy(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.archive(root / 'source.tar.xz', [('flac-1.5.0/a', tarfile.REGTYPE, b'bounded')])
            with patch.object(proof, 'MAX_SOURCE', 3), self.assertRaisesRegex(ValueError, 'source limit'):
                proof.unpack(root / 'source.tar.xz', root / 'source')

    def test_non_elf_and_wrong_native_version_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory) / 'libFLAC.so.14'
            library.write_bytes(b'ambient runtime')
            with self.assertRaisesRegex(ValueError, 'ELF'):
                proof.runtime_version(library)
            raw = bytearray(64)
            raw[:6] = b'\x7fELF\x02\x01'
            raw[18:20] = b'\x3e\x00'
            library.write_bytes(raw)
            class Version:
                value = b'1.4.3'
            with patch.object(proof.ctypes, 'CDLL'), patch.object(proof.ctypes, 'c_char_p') as pointer:
                pointer.in_dll.return_value = Version()
                with self.assertRaisesRegex(ValueError, 'exactly 1.5.0'):
                    proof.runtime_version(library)

    def test_https_redirect_cannot_downgrade(self):
        with self.assertRaisesRegex(ValueError, 'HTTPS'):
            proof.HttpsRedirect().redirect_request(None, None, 302, '', {}, 'http://example.test/file')


if __name__ == '__main__':
    unittest.main()
