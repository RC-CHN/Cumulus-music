"""Archive handoff regressions with no package installation or network access."""

import hashlib
import os
from pathlib import Path
import tempfile
import unittest

from linux_webview_apt import MAX_DEB, admit, apt_plan, receipt, stage


IDENTITY = {'commit': 'a' * 40, 'ciRun': '123', 'ciAttempt': '2'}
URL = 'https://archive.ubuntu.com/ubuntu/pool/main/t/test/test_1_amd64.deb'


def uri_row(name, payload, size=None):
    sha256 = hashlib.sha256(payload).hexdigest()
    return f"'{URL}' {name} {len(payload) if size is None else size} SHA256:{sha256}\n".encode()


class LinuxWebViewApt(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.artifact = self.root / 'artifact'
        self.artifact.mkdir()
        self.staging = self.root / 'staging'
        self.staging.mkdir()
        (self.artifact / 'receipt.json').write_bytes(receipt(IDENTITY))

    def test_exact_apt_filename_can_include_a_percent_encoded_epoch(self):
        name = 'ffmpeg_7%3a6.1.1-3ubuntu5_amd64.deb'
        package = b'trusted archive'
        plan = apt_plan(b'Reading package lists...\n' + uri_row(name, package))
        self.assertEqual(plan, {name: (len(package), hashlib.sha256(package).hexdigest())})

    def test_wrong_same_length_bytes_wrong_size_and_special_files_are_misses(self):
        package = b'1234'
        names = [f'{part}_1_amd64.deb' for part in ('good', 'digest', 'size', 'link', 'fifo', 'missing')]
        plan = apt_plan(b''.join(uri_row(name, package) for name in names))
        for name, data in zip(names, [package, b'5678', package + b'5']):
            (self.artifact / name).write_bytes(data)
        (self.artifact / names[3]).symlink_to(self.artifact / names[0])
        os.mkfifo(self.artifact / names[4])
        (self.artifact / 'unexpected_1_all.deb').write_bytes(package)
        accepted = admit(self.artifact, IDENTITY, plan, self.staging)
        self.assertEqual(accepted, [self.staging / names[0]])
        self.assertEqual(list(self.staging.iterdir()), accepted)
        (self.artifact / names[0]).write_bytes(b'abcd')
        self.assertEqual(accepted[0].read_bytes(), package)

    def test_missing_or_wrong_attempt_receipt_admits_nothing(self):
        name = 'audio_1_amd64.deb'
        payload = b'expected'
        (self.artifact / name).write_bytes(payload)
        plan = apt_plan(uri_row(name, payload))
        manifest = self.artifact / 'receipt.json'
        for contents in (None, receipt({**IDENTITY, 'ciAttempt': '1'}), b'x' * 1025):
            with self.subTest(contents=contents):
                manifest.unlink(missing_ok=True)
                if contents is not None:
                    manifest.write_bytes(contents)
                self.assertEqual(admit(self.artifact, IDENTITY, plan, self.staging), [])
                self.assertEqual(list(self.staging.iterdir()), [])
        symlink = self.root / 'outside-link'
        symlink.symlink_to(self.artifact, target_is_directory=True)
        manifest.write_bytes(receipt(IDENTITY))
        self.assertEqual(admit(symlink, IDENTITY, plan, self.staging), [])

    def test_bad_receipt_plan_cannot_authorize_any_package(self):
        row = uri_row('test_1_all.deb', b'pkg')
        for data in (
            row + row,
            row + uri_row('../other_1_all.deb', b'pkg'),
            row + uri_row('/tmp/other_1_all.deb', b'pkg'),
            row.replace(b'SHA256:', b'MD5Sum:'),
            uri_row('oversize_1_all.deb', b'x', MAX_DEB + 1),
        ):
            with self.subTest(data=data), self.assertRaises(ValueError):
                apt_plan(data)

    def test_producer_retains_only_downloaded_archive_and_consumer_checks_it(self):
        archives = self.root / 'apt'
        archives.mkdir()
        (archives / 'partial').mkdir()
        (archives / 'lock').touch()
        name = 'cache_1_amd64.deb'
        package = b'package archive'
        (archives / name).write_bytes(package)
        published = self.root / 'published'
        self.assertEqual(stage(archives, published, IDENTITY), (1, len(package)))
        self.assertEqual({p.name for p in published.iterdir()}, {name, 'receipt.json'})
        plan = apt_plan(uri_row(name, package))
        self.assertEqual(admit(published, IDENTITY, plan, self.staging), [self.staging / name])


if __name__ == '__main__':
    unittest.main()
