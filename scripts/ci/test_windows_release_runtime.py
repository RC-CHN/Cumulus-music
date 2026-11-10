"""The early Windows WebView runtime and portable bundle must match byte for byte."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

from windows_release_runtime import FILTER, MANIFEST, MEDIA_PROOF, RUNTIME, receipt, restore, verify_portable
from windows_webview_inputs import ci_identity, file_receipt


class SharedWindowsRuntime(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.artifact = self.root / 'artifact'
        self.runtime = self.artifact / RUNTIME
        self.runtime.mkdir(parents=True)
        for name in ('libmpv-2.dll', 'libFLAC.dll', 'ffprobe.exe', FILTER, 'third-party/licenses/notice'):
            path = self.runtime / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(name.encode())
        media_proof = self.runtime / MEDIA_PROOF
        media_proof.parent.mkdir()
        media_proof.write_text(json.dumps({
            'filterLoaded': True, 'nativePcmVerified': True,
            'pluginSha256': file_receipt(self.runtime / FILTER)['sha256'],
        }), encoding='utf-8')
        self.identity = ci_identity(dict(GITHUB_SHA='a' * 40, GITHUB_RUN_ID='123', GITHUB_RUN_ATTEMPT='2'))
        self.dlls = ['libflac.dll', 'libmpv-2.dll']
        self.flac = {'fixture': True}
        decoder = file_receipt(self.runtime / 'libFLAC.dll')
        patched = patch('windows_release_runtime.decoder_members',
                        return_value={'libFLAC.dll': (decoder['bytes'], decoder['sha256'])})
        patched.start()
        self.addCleanup(patched.stop)
        self.manifest = receipt(self.runtime, self.identity, self.dlls, self.flac)
        (self.artifact / MANIFEST).write_text(json.dumps(self.manifest), encoding='utf-8')

    def test_same_attempt_feeds_early_webview_and_package(self):
        gui = self.root / 'gui'
        packaged = self.root / 'packaged'
        restore(self.artifact, self.identity, gui, include_notices=False)
        restore(self.artifact, self.identity, packaged, include_notices=True)
        self.assertFalse((gui / 'third-party').exists())
        self.assertEqual({path.name for path in gui.iterdir()}, {
            'libmpv-2.dll', 'libFLAC.dll', 'ffprobe.exe', 'cumulus_audio_filter.dll',
        })
        for path in gui.iterdir():
            self.assertEqual(path.read_bytes(), (packaged / path.name).read_bytes())
        self.assertTrue((packaged / 'third-party/licenses/notice').is_file())

    def test_wrong_attempt_or_changed_bytes_never_reach_destination(self):
        destination = self.root / 'untrusted'
        with self.assertRaisesRegex(AssertionError, 'provenance'):
            restore(self.artifact, self.identity | {'ciAttempt': '1'}, destination, include_notices=True)
        (self.runtime / 'libmpv-2.dll').write_bytes(b'Xibmpv-2.dll')
        with self.assertRaisesRegex(AssertionError, 'checksum'):
            restore(self.artifact, self.identity, destination, include_notices=True)
        self.assertFalse(destination.exists())

    def test_runtime_rejects_an_unverified_filter_even_with_new_archive_hashes(self):
        (self.runtime / FILTER).write_bytes(b'different release filter')
        with self.assertRaisesRegex(AssertionError, 'differs from the runtime-verified filter'):
            receipt(self.runtime, self.identity, self.dlls, self.flac)

    def test_portable_archive_must_match_the_runtime_tested_by_webview(self):
        archive = self.root / 'portable.zip'
        for changed in (False, True):
            with zipfile.ZipFile(archive, 'w') as bundle:
                for name in self.manifest['files']:
                    data = (self.runtime / name).read_bytes()
                    if changed and name == 'ffprobe.exe':
                        data = b'!' + data[1:]
                    bundle.writestr('Cumulus-fixture/' + name, data)
            if changed:
                with self.assertRaisesRegex(AssertionError, 'checksum'):
                    verify_portable(archive, 'Cumulus-fixture', self.manifest)
            else:
                verify_portable(archive, 'Cumulus-fixture', self.manifest)


if __name__ == '__main__':
    unittest.main()
