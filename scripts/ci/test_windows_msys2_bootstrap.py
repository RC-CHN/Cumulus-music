"""Exercise the Windows tool cache transaction with small synthetic transfers."""
import contextlib
import hashlib
import io
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch
import urllib.error
import urllib.request

import windows_msys2_bootstrap as bootstrap


INSTALLER = b'synthetic installer'
WRONG_BYTES = b'corrupted installer'
MIRROR, RELEASE = (url for _, url in bootstrap.SOURCES)


class Response(io.BytesIO):
    status = 200

    def __init__(self, body, url, *, content_length=True):
        super().__init__(body)
        self.url = url
        self.headers = {'Content-Length': str(len(body))} if content_length else {}

    def geturl(self):
        return self.url


class WindowsMsys2Bootstrap(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.cache = self.root / 'msys2-installer' / '0.0.20260927' / 'x64'
        self.installer = self.cache / 'base.exe'
        self.marker = self.cache.with_suffix('.complete')
        self.enterContext(patch.multiple(
            bootstrap,
            ARCHIVE_BYTES=len(INSTALLER),
            ARCHIVE_SHA256=hashlib.sha256(INSTALLER).hexdigest(),
        ))
        self.enterContext(contextlib.redirect_stdout(io.StringIO()))

    def assert_ready(self):
        self.assertEqual(self.installer.read_bytes(), INSTALLER)
        self.assertEqual(self.marker.read_bytes(), b'')
        self.assertEqual(list(self.cache.iterdir()), [self.installer])

    def test_mirror_gateway_timeout_uses_pinned_release_and_publishes_marker_last(self):
        def fetch(url, *, timeout):
            self.assertFalse(self.marker.exists())
            self.assertFalse(self.installer.exists())
            if url == MIRROR:
                raise urllib.error.HTTPError(url, 504, 'synthetic gateway timeout', {}, None)
            self.assertEqual(url, RELEASE)
            return Response(INSTALLER, 'https://release-assets.githubusercontent.com/pinned-asset')

        def replace(source, destination):
            if destination == self.marker:
                self.assertEqual(self.installer.read_bytes(), INSTALLER)
                self.assertFalse(self.marker.exists())
            replace_file(source, destination)

        replace_file = os.replace
        opener = Mock(side_effect=fetch)
        with patch.object(bootstrap.os, 'replace', side_effect=replace) as publish:
            bootstrap.seed(self.root, opener)
        self.assertEqual([call.args[0] for call in opener.call_args_list], [MIRROR, RELEASE])
        self.assertEqual([call.args[1] for call in publish.call_args_list], [self.installer, self.marker])
        self.assert_ready()

    def test_matching_ready_cache_is_rehashed_without_network(self):
        self.cache.mkdir(parents=True)
        self.installer.write_bytes(INSTALLER)
        self.marker.touch()
        opener = Mock()
        with patch.object(bootstrap, 'sha256', wraps=bootstrap.sha256) as checksum:
            bootstrap.seed(self.root, opener)
        checksum.assert_called_once_with(self.installer)
        opener.assert_not_called()
        self.assert_ready()

    def test_corrupt_ready_cache_invalidated_before_redownload(self):
        self.cache.mkdir(parents=True)
        self.assertEqual(len(WRONG_BYTES), len(INSTALLER))
        self.installer.write_bytes(WRONG_BYTES)
        self.marker.touch()

        def fetch(url, *, timeout):
            self.assertEqual(url, MIRROR)
            self.assertFalse(self.marker.exists())
            return Response(INSTALLER, url)

        bootstrap.seed(self.root, fetch)
        self.assert_ready()

    def test_verified_file_left_without_marker_recovers_without_network(self):
        self.cache.mkdir(parents=True)
        self.installer.write_bytes(INSTALLER)
        opener = Mock()
        bootstrap.seed(self.root, opener)
        opener.assert_not_called()
        self.assert_ready()

    def test_bad_mirror_digest_fails_without_approval_or_second_source(self):
        opener = Mock(return_value=Response(WRONG_BYTES, MIRROR))
        with self.assertRaisesRegex(bootstrap.BootstrapError, 'SHA-256'):
            bootstrap.seed(self.root, opener)
        self.assertEqual([call.args[0] for call in opener.call_args_list], [MIRROR])
        self.assertFalse(self.marker.exists())
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_unbounded_body_is_rejected_even_without_content_length(self):
        opener = Mock(return_value=Response(INSTALLER + b'extra', MIRROR, content_length=False))
        with self.assertRaisesRegex(bootstrap.BootstrapError, 'pinned byte length'):
            bootstrap.seed(self.root, opener)
        self.assertFalse(self.marker.exists())
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_interrupted_partial_mirror_cannot_mix_bytes_into_release(self):
        class InterruptedResponse(Response):
            def read1(self, size):
                if self.tell() != 0:
                    raise ConnectionResetError
                return super().read1(min(size, 5))

        opener = Mock(side_effect=[InterruptedResponse(WRONG_BYTES, MIRROR), Response(INSTALLER, RELEASE)])
        bootstrap.seed(self.root, opener)
        self.assertEqual([call.args[0] for call in opener.call_args_list], [MIRROR, RELEASE])
        self.assert_ready()

    def test_early_eof_with_pinned_content_length_uses_release(self):
        truncated = Response(WRONG_BYTES[:5], MIRROR)
        truncated.headers['Content-Length'] = str(len(INSTALLER))
        opener = Mock(side_effect=[truncated, Response(INSTALLER, RELEASE)])
        bootstrap.seed(self.root, opener)
        self.assertEqual([call.args[0] for call in opener.call_args_list], [MIRROR, RELEASE])
        self.assert_ready()

    def test_redirect_hook_rejects_downgrade_and_other_authorities(self):
        request = urllib.request.Request(MIRROR)
        redirect = bootstrap.OfficialRedirect()
        for target in ('http://repo.msys2.org/file', 'https://downloads.example.invalid/file',
                       'https://username@repo.msys2.org/file', 'https://repo.msys2.org:8443/file'):
            with self.subTest(target=target), self.assertRaisesRegex(bootstrap.BootstrapError, 'official HTTPS'):
                redirect.redirect_request(request, None, 302, 'Found', {}, target)


if __name__ == '__main__':
    unittest.main()
