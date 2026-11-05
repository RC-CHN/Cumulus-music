import hashlib
import io
from pathlib import Path
import ssl
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

import linux_flac150 as proof


class DownloadBoundaries(unittest.TestCase):
    payload = b'pinned FLAC archive fixture'

    class Response(io.BytesIO):
        def __init__(self, data, *, url=proof.URL, headers=None):
            super().__init__(data)
            self.url = url
            self.headers = headers or {}

        def geturl(self):
            return self.url

    def setUp(self):
        work = tempfile.TemporaryDirectory()
        self.addCleanup(work.cleanup)
        self.path = Path(work.name) / 'flac-1.5.0.tar.xz'
        self.enterContext(patch.object(proof, 'ARCHIVE_SHA256', hashlib.sha256(self.payload).hexdigest()))
        self.opener = self.enterContext(patch.object(proof.urllib.request, 'build_opener')).return_value
        self.sleep = self.enterContext(patch.object(proof.time, 'sleep'))

    def test_tls_eof_retries_same_source_then_publishes_only_verified_bytes(self):
        eof = urllib.error.URLError(ssl.SSLEOFError(8, 'synthetic TLS handshake EOF'))
        self.opener.open.side_effect = [eof, self.Response(self.payload)]
        proof.download(self.path)
        self.assertEqual(self.path.read_bytes(), self.payload)
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])
        self.assertEqual([call.args for call in self.opener.open.call_args_list], [(proof.URL,)] * 2)
        self.sleep.assert_called_once_with(1)

    def test_interrupted_partial_bytes_are_discarded_before_each_retry(self):
        class Broken(self.Response):
            def read(self, _size):
                if self.tell():
                    raise TimeoutError('synthetic body interruption')
                return super().read(2)

        def open_broken(*_args, **_kwargs):
            self.assertEqual(list(self.path.parent.iterdir()), [])
            return Broken(self.payload)

        self.opener.open.side_effect = open_broken
        with self.assertRaisesRegex(TimeoutError, 'body interruption'):
            proof.download(self.path)
        self.assertEqual(self.opener.open.call_count, 3)
        self.assertEqual(list(self.path.parent.iterdir()), [])
        self.assertEqual([call.args for call in self.sleep.call_args_list], [(1,), (2,)])

    def test_transient_http_error_cannot_retry_past_the_total_budget(self):
        self.opener.open.side_effect = urllib.error.HTTPError(proof.URL, 503, 'unavailable', {}, None)
        with patch.object(proof.time, 'monotonic', side_effect=[0, 0, 89.5]):
            with self.assertRaises(urllib.error.HTTPError):
                proof.download(self.path)
        self.opener.open.assert_called_once_with(proof.URL, timeout=30)
        self.sleep.assert_not_called()
        self.assertEqual(list(self.path.parent.iterdir()), [])

    def test_bad_content_and_non_transient_policy_failures_are_not_retried(self):
        for response, expected in [
            (self.Response(b'wrong digest'), ValueError),
            (self.Response(self.payload, url='http://example.invalid/file'), ValueError),
            (self.Response(self.payload, headers={'Content-Length': '-2'}), ValueError),
            (self.Response(self.payload, headers={'Content-Length': '300'}), ValueError),
            (urllib.error.HTTPError(proof.URL, 404, 'missing', {}, None), urllib.error.HTTPError),
            (urllib.error.URLError(ssl.SSLCertVerificationError('untrusted')), urllib.error.URLError),
        ]:
            with self.subTest(response=response, expected=expected):
                self.opener.open.reset_mock(side_effect=True)
                self.opener.open.side_effect = [response]
                with self.assertRaises(expected):
                    proof.download(self.path)
                self.opener.open.assert_called_once()
                self.sleep.assert_not_called()
                self.assertEqual(list(self.path.parent.iterdir()), [])

    def test_existing_and_concurrent_destinations_are_not_replaced(self):
        self.path.write_bytes(b'already there')
        with self.assertRaises(FileExistsError):
            proof.download(self.path)
        self.opener.open.assert_not_called()
        self.path.unlink()

        def open_concurrent(*_args, **_kwargs):
            self.path.write_bytes(b'concurrent source')
            return self.Response(self.payload)

        self.opener.open.side_effect = open_concurrent
        with self.assertRaises(FileExistsError):
            proof.download(self.path)
        self.assertEqual(self.path.read_bytes(), b'concurrent source')
        self.assertEqual(list(self.path.parent.iterdir()), [self.path])


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
