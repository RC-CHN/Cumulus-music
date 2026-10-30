import os
from pathlib import Path
import tarfile
import tempfile
import unittest

from diagnostics import archive


class ProofArchive(unittest.TestCase):
    def test_preserves_fixture_names_and_excludes_unrelated_build_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            name = "滤镜 [test], quote'" + (' colon:' if os.name != 'nt' else '')
            proof = root / 'package-stage/linux-x64/equalizer-proof' / name / 'filter.txt'
            proof.parent.mkdir(parents=True)
            proof.write_text('synthetic', encoding='utf-8')
            (root / 'ci-engine-roundtrip-windows.log').write_text('stage engine_start')
            (root / 'private-session').write_text('must not be archived')
            target = root / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(root, target), 2)
            with tarfile.open(target) as saved:
                self.assertEqual(saved.extractfile(proof.relative_to(root).as_posix()).read(), b'synthetic')
                self.assertNotIn('private-session', saved.getnames())
                self.assertEqual(saved.extractfile('ci-engine-roundtrip-windows.log').read(), b'stage engine_start')

    def test_early_failure_can_archive_an_empty_proof_set(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            target = root / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(root, target), 0)
            with tarfile.open(target) as saved:
                self.assertEqual(saved.getnames(), [])

    def test_download_fixture_failure_and_neighboring_ui_logs_are_retained(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = ['ci-download-page-linux/first.log', 'ci-download-menu-windows/first/failed.json',
                     'ci-playlist-menu-linux/host.log', 'ci-song-menu-linux/audit.json',
                     'ci-search-results-linux/first/failed.json']
            for relative in paths:
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('synthetic', encoding='utf-8')
            (root / 'private-download-session').write_text('excluded')
            target = root / 'diagnostics/proofs.tar.gz'
            self.assertEqual(archive(root, target), len(paths))
            with tarfile.open(target) as saved:
                for relative in paths:
                    self.assertEqual(saved.extractfile(relative).read(), b'synthetic')
                self.assertNotIn('private-download-session', saved.getnames())


if __name__ == '__main__':
    unittest.main()
