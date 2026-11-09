import os
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from voice_song_download_smoke import published_mp3


class PublishedMp3Tests(unittest.TestCase):
    def setUp(self):
        scratch = TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name) / 'download-output'
        self.root.mkdir()
        self.target = self.root / '合成.mp3'
        self.target.write_bytes(b'synthetic fixture')

    def test_direct_file(self):
        self.assertEqual(published_mp3(self.target, self.root).read_bytes(), b'synthetic fixture')

    @unittest.skipUnless(os.name == 'nt', 'extended-length paths need the Windows filesystem')
    def test_real_windows_extended_path_has_same_direct_parent(self):
        normal = str(self.target.resolve(strict=True))
        self.assertFalse(normal.startswith('\\\\?\\'), normal)
        extended = Path('\\\\?\\UNC\\' + normal[2:] if normal.startswith('\\\\') else '\\\\?\\' + normal)
        self.assertNotEqual(extended.resolve(strict=True).parent, self.root.resolve(strict=True))
        self.assertTrue(published_mp3(extended, self.root).samefile(self.target))

    def test_rejects_nested_and_wrong_destination(self):
        nested = self.root / 'nested'
        nested.mkdir()
        nested_file = nested / '合成.mp3'
        nested_file.write_bytes(self.target.read_bytes())
        for file, directory in ((nested_file, self.root), (self.target, nested)):
            with self.subTest(file=file, directory=directory):
                with self.assertRaisesRegex(AssertionError, 'unexpected download parent'):
                    published_mp3(file, directory)

    def test_rejects_wrong_format_and_nonfile(self):
        wrong_suffix = self.root / '合成.flac'
        wrong_suffix.write_bytes(self.target.read_bytes())
        with self.assertRaisesRegex(AssertionError, 'unexpected download format'):
            published_mp3(wrong_suffix, self.root)
        directory = self.root / '目录.mp3'
        directory.mkdir()
        empty = self.root / '空.mp3'
        empty.touch()
        for target in (directory, empty):
            with self.subTest(target=target):
                with self.assertRaisesRegex(AssertionError, 'not a nonempty ordinary file'):
                    published_mp3(target, self.root)

    def test_rejects_redirected_destination_and_file(self):
        alias = self.root.with_name('redirected')
        redirected = self.root / 'redirected.mp3'
        try:
            alias.symlink_to(self.root, target_is_directory=True)
            redirected.symlink_to(self.target)
        except OSError as error:
            self.skipTest(f'OS could not create synthetic links: {error}')
        with self.assertRaisesRegex(AssertionError, 'destination is not an ordinary directory'):
            published_mp3(alias / self.target.name, alias)
        with self.assertRaisesRegex(AssertionError, 'not a nonempty ordinary file'):
            published_mp3(redirected, self.root)


if __name__ == '__main__':
    unittest.main()
