import hashlib
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
from windows_media_recipe import download


class SourceDownload(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.target = Path(self.directory.name) / 'source.tar.xz'
        self.expected = hashlib.sha256(b'pinned source').hexdigest()

    def test_transient_connection_then_success_publishes_only_verified_bytes(self):
        with patch('windows_media_recipe.urllib.request.urlopen', side_effect=[urllib.error.URLError('timeout'), io.BytesIO(b'pinned source')]) as network, patch('windows_media_recipe.time.sleep'):
            download('https://example.invalid/source', self.target, self.expected)
        self.assertEqual(network.call_count, 2)
        self.assertEqual(self.target.read_bytes(), b'pinned source')
        self.assertEqual(list(self.target.parent.glob('*.part')), [])

    def test_exhausted_partial_response_never_leaves_an_input_or_temporary_file(self):
        class Broken(io.BytesIO):
            def read(self, size=-1):
                if self.tell():
                    raise TimeoutError('synthetic interrupted transfer')
                return super().read(2)
        with patch('windows_media_recipe.urllib.request.urlopen', side_effect=lambda *a, **k: Broken(b'pinned source')) as network, patch('windows_media_recipe.time.sleep'):
            with self.assertRaises(TimeoutError):
                download('https://example.invalid/source', self.target, self.expected)
        self.assertEqual(network.call_count, 3)
        self.assertEqual(list(self.target.parent.iterdir()), [])

    def test_checksum_mismatch_and_http_not_found_are_not_retried(self):
        for response, error in [(io.BytesIO(b'corrupt source'), ValueError), (urllib.error.HTTPError('https://example.invalid/source', 404, 'missing', {}, None), urllib.error.HTTPError)]:
            with patch('windows_media_recipe.urllib.request.urlopen', side_effect=response if isinstance(response, Exception) else None, return_value=response) as network:
                with self.assertRaises(error):
                    download('https://example.invalid/source', self.target, self.expected)
            self.assertEqual(network.call_count, 1)
            self.assertEqual(list(self.target.parent.iterdir()), [])

    def test_existing_input_is_preserved_without_a_network_request(self):
        self.target.write_bytes(b'keep existing')
        with patch('windows_media_recipe.urllib.request.urlopen') as network:
            with self.assertRaises(FileExistsError):
                download('https://example.invalid/source', self.target, self.expected)
        network.assert_not_called()
        self.assertEqual(self.target.read_bytes(), b'keep existing')


if __name__ == '__main__':
    unittest.main()
