import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import windows_flac as flac


def digest(data):
    return hashlib.sha256(data).hexdigest()


class WindowsFlacStageTests(unittest.TestCase):
    def stage(self, *, notice_changed=False, wrong_pe=False):
        with tempfile.TemporaryDirectory() as directory:
            msys = Path(directory) / 'msys'
            ambient = msys / 'ucrt64/bin' / flac.FLAC
            ambient.parent.mkdir(parents=True)
            ambient.write_bytes(b'wrong installed FLAC')
            stage = Path(directory) / 'stage'
            stage.mkdir()
            binary, notice = b'synthetic DLL', b'synthetic Xiph notice'
            imports = set(flac.PE_IMPORTS)
            if wrong_pe:
                imports.remove('libogg-0.dll')
            metadata = 'file format pei-x86-64\n' + '\n'.join(f'DLL Name: {name}' for name in sorted(imports))
            lock = dict(FLAC_DLL_BYTES=len(binary), FLAC_DLL_SHA256=digest(binary),
                        FLAC_LICENSE_BYTES=len(notice), FLAC_LICENSE_SHA256=digest(notice))
            tar_outputs = ['/path/to/signed-package\n', binary,
                           b'0' + notice[1:] if notice_changed else notice, metadata]
            with patch.multiple(flac, **lock), patch.object(flac, 'download') as download, \
                    patch.object(flac.subprocess, 'check_output', side_effect=tar_outputs) as execute:
                if notice_changed or wrong_pe:
                    with self.assertRaisesRegex(ValueError, 'archive member failed' if notice_changed else 'architecture or imports'):
                        flac.stage_decoder(stage, msys)
                    self.assertEqual(list(stage.iterdir()), [])
                    return
                actual = flac.stage_decoder(stage, msys)
                self.assertEqual(actual, flac.PE_IMPORTS)
                url, archive, sha = download.call_args.args
                self.assertEqual((url, archive.name, sha), (flac.FLAC_URL, flac.FLAC_ARCHIVE, flac.FLAC_ARCHIVE_SHA256))
                self.assertEqual(
                    [call.args[0][3] for call in execute.call_args_list[1:3]],
                    [f'ucrt64/bin/{flac.FLAC}', 'ucrt64/share/licenses/flac/COPYING.Xiph'],
                )
                self.assertEqual((stage / flac.FLAC).read_bytes(), binary)
                self.assertEqual((stage / flac.FLAC_LICENSE).read_bytes(), notice)
                self.assertEqual(ambient.read_bytes(), b'wrong installed FLAC')
                with self.assertRaisesRegex(ValueError, 'missing or linked libFLAC dependency'):
                    flac.decoder_receipt(stage, actual)
                for name in flac.FLAC_IMPORTS:
                    (stage / name).write_bytes(name.encode())
                receipt = flac.decoder_receipt(stage, actual)
                self.assertEqual(flac.decoder_members(receipt)['libogg-0.dll'],
                                 (len('libogg-0.dll'), digest(b'libogg-0.dll')))

    def test_only_locked_members_enter_the_release_stage(self):
        self.stage()

    def test_changed_notice_never_publishes_the_decoder(self):
        self.stage(notice_changed=True)

    def test_unexpected_pe_import_never_publishes_the_decoder(self):
        self.stage(wrong_pe=True)


if __name__ == '__main__':
    unittest.main()
