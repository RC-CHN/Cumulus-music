import hashlib
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import installed_linux_smoke as smoke
import linux_flac_artifacts as artifacts


LIBRARY = b'synthetic artifact bytes; not a real ELF'
NOTICE = b'synthetic license for archive validation'
META = {'platform': 'linux-x64', 'version': '0.0.0', 'commit': 'a' * 40}


def sha(data):
    return hashlib.sha256(data).hexdigest()


def receipt():
    return {
        'version': artifacts.VERSION, 'sourceUrl': artifacts.URL,
        'sourceArchiveSha256': artifacts.ARCHIVE_SHA256,
        'library': {'name': artifacts.LIBRARY, 'bytes': len(LIBRARY), 'sha256': sha(LIBRARY)},
        'license': {'path': artifacts.NOTICE, 'sha256': sha(NOTICE)},
        'elf': {'soname': artifacts.LIBRARY, 'needed': sorted(artifacts.candidate.SYSTEM_IMPORTS),
                'symbolicBinding': True, 'flacSymbolRelocations': 0, 'searchTags': []},
        'native': {'version': artifacts.VERSION, 'libraryIdentityVerified': True,
                   'decoderAllocatedAndDeleted': True, 'loaderTraceVerified': True,
                   'foreignFlacSymbolBindings': 0},
    }


def regular(name, body):
    entry = tarfile.TarInfo(name)
    entry.size = len(body)
    entry.mode = 0o755
    return entry, io.BytesIO(body)


class FlacPackagePayloads(unittest.TestCase):
    def setUp(self):
        mock_notice = patch.object(artifacts.candidate, 'NOTICE_SHA256', sha(NOTICE))
        mock_notice.start()
        self.addCleanup(mock_notice.stop)

    def test_verify_complete_portable_receipt_and_bytes(self):
        runtime = receipt()
        root = 'Cumulus-portable'
        entries = [regular(f'{root}/{artifacts.LIBRARY}', LIBRARY),
                   regular(f'{root}/{artifacts.NOTICE}', NOTICE),
                   regular(f'{root}/runtime-manifest.json',
                           json.dumps({**META, 'flacDecoder': runtime}).encode())]
        with tempfile.TemporaryDirectory() as temp:
            tar = Path(temp) / 'portable.tar.gz'
            with tarfile.open(tar, 'w:gz') as package:
                for member, data in entries:
                    package.addfile(member, data)
            with tarfile.open(tar, 'r|gz') as package:
                artifacts.verify_members(package, root, runtime, portable_metadata=META)

    def test_reject_incomplete_linked_substituted_or_inconsistent_artifact(self):
        runtime = receipt()
        root = 'Cumulus-portable'
        library, notice, manifest = (f'{root}/{name}' for name in
                                     (artifacts.LIBRARY, artifacts.NOTICE, 'runtime-manifest.json'))

        def verify(entries):
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode='w:gz') as package:
                for member, body in entries:
                    package.addfile(member, body)
            data.seek(0)
            with tarfile.open(fileobj=data, mode='r|gz') as package:
                artifacts.verify_members(package, root, runtime, portable_metadata=META)

        def normal_manifest(**changes):
            return regular(manifest, json.dumps({**META, 'flacDecoder': runtime, **changes}).encode())

        link = tarfile.TarInfo(library)
        link.type = tarfile.SYMTYPE
        link.linkname = '/usr/lib/libFLAC.so.14'
        variants = {
            'missing library': [regular(notice, NOTICE), normal_manifest()],
            'wrong same-sized library': [regular(library, b'x' * len(LIBRARY)),
                                         regular(notice, NOTICE), normal_manifest()],
            'wrong notice': [regular(library, LIBRARY), regular(notice, b'wrong'), normal_manifest()],
            'linked library': [(link, None), regular(notice, NOTICE), normal_manifest()],
            'wrong stage identity': [regular(library, LIBRARY), regular(notice, NOTICE),
                                     normal_manifest(commit='b' * 40)],
            'inconsistent portable receipt': [regular(library, LIBRARY), regular(notice, NOTICE),
                                              normal_manifest(flacDecoder=None)],
            'duplicate library': [regular(library, LIBRARY), regular(library, LIBRARY),
                                  regular(notice, NOTICE), normal_manifest()],
        }
        for name, entries in variants.items():
            with self.subTest(name=name), self.assertRaises(ValueError):
                verify(entries)

    def test_portable_layout_copies_known_package_members_and_legacy_mode_stays_available(self):
        runtime = receipt()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            tar = root / 'portable.tar.gz'
            prefix = 'Cumulus-portable'
            with tarfile.open(tar, 'w:gz') as package:
                for path, data in [('cumulus', b'executable'), ('libcumulus_audio_filter.so', b'filter'),
                                   (artifacts.LIBRARY, LIBRARY), (artifacts.NOTICE, NOTICE)]:
                    member, body = regular(f'{prefix}/{path}', data)
                    package.addfile(member, body)
            ordinary = root / 'ordinary'
            smoke.unpack_portable(tar, ordinary, prefix)
            self.assertFalse((ordinary / artifacts.LIBRARY).exists())
            extracted = root / 'packaged'
            smoke.unpack_portable(tar, extracted, prefix, flac_decoder=runtime)
            staged = root / artifacts.LIBRARY
            staged.write_bytes(LIBRARY)
            observed = artifacts.verify_extracted(extracted, runtime, staged_library=staged)
            self.assertEqual(observed['sha256'], sha(LIBRARY))
            self.assertTrue(observed['matchesPackageStage'])
            staged.write_bytes(b'replaced')
            with self.assertRaisesRegex(ValueError, 'this package stage'):
                artifacts.verify_extracted(extracted, runtime, staged_library=staged)


if __name__ == '__main__':
    unittest.main()
