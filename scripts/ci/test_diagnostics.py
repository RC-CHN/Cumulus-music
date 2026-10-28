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


if __name__ == '__main__':
    unittest.main()
