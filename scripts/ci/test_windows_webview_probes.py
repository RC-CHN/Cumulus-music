"""Exact-artifact boundaries for the Windows WebView shard build handoff."""
from collections import Counter
from pathlib import Path
import re
import shutil
import tempfile
import unittest

from windows_webview_inputs import UX, ci_identity, frontend_receipts
from windows_webview_probes import (
    ARTIFACT, COLD_FLAC_TEST, EXAMPLES, FRONTEND, MANIFEST, PROBES,
    binary_names, restore, stage,
)


class WindowsWebViewProbes(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        self.producer = root / 'producer'
        self.consumer = root / 'consumer'
        self.consumer.mkdir()
        self.identity = ci_identity(dict(GITHUB_SHA='a' * 40, GITHUB_RUN_ID='123', GITHUB_RUN_ATTEMPT='2'))
        dist = self.producer / FRONTEND
        dist.mkdir(parents=True)
        (dist / 'index.html').write_text('<title>Exact CI frontend</title>', encoding='utf-8')
        (dist / 'script.js').write_bytes(b'synthetic exact frontend bytes')
        shutil.copytree(dist, self.consumer / FRONTEND)
        examples = self.producer / EXAMPLES
        examples.mkdir(parents=True)
        for shard in PROBES:
            for name in binary_names(shard):
                (examples / name).write_bytes(name.encode())
        stage(self.producer, self.identity, frontend_receipts(dist))
        self.shard = 'playback'
        self.staged = self.consumer / ARTIFACT / self.shard
        self.staged.parent.mkdir(parents=True)
        (self.producer / ARTIFACT / self.shard).rename(self.staged)

    def test_restores_only_verified_shard_and_leaves_shared_ux(self):
        ux = self.consumer / UX
        ux.parent.mkdir(parents=True)
        ux.write_bytes(b'shared probe')
        restore(self.consumer, self.identity, self.shard)
        examples = self.consumer / EXAMPLES
        self.assertEqual({p.name for p in examples.iterdir()}, {UX.name, *binary_names(self.shard)})
        self.assertEqual(ux.read_bytes(), b'shared probe')
        self.assertEqual({p.name for p in self.staged.iterdir()}, {MANIFEST})
        for name in binary_names(self.shard):
            self.assertEqual((examples / name).read_bytes(), name.encode())

    def test_wrong_ci_identity_wrong_frontend_or_tampering_promotes_nothing(self):
        def reject(identity):
            with self.assertRaisesRegex(AssertionError, 'provenance or checksum'):
                restore(self.consumer, identity, self.shard)
            self.assertFalse((self.consumer / EXAMPLES).exists())

        for field, value in [('commit', 'b' * 40), ('ciRun', '124'), ('ciAttempt', '1')]:
            with self.subTest(field=field):
                reject(self.identity | {field: value})
        for path in (self.staged / binary_names(self.shard)[0],
                     self.consumer / FRONTEND / 'script.js'):
            with self.subTest(path=path.name):
                original = path.read_bytes()
                path.write_bytes(b'!' + original[1:])
                reject(self.identity)
                path.write_bytes(original)

    def test_foreign_missing_or_extra_executable_is_not_promoted(self):
        foreign = binary_names('catalog')[0]
        path = self.staged / binary_names(self.shard)[0]
        for mutate, reset in (
            (lambda: path.rename(path.with_name(foreign)),
             lambda: path.with_name(foreign).rename(path)),
            (lambda: path.rename(self.staged.parent / path.name),
             lambda: (self.staged.parent / path.name).rename(path)),
            (lambda: path.with_name(foreign).write_bytes(b'extra'),
             lambda: path.with_name(foreign).unlink()),
        ):
            mutate()
            with self.assertRaisesRegex(AssertionError, 'unexpected WebView probe contents'):
                restore(self.consumer, self.identity, self.shard)
            self.assertFalse((self.consumer / EXAMPLES).exists())
            reset()

    def test_existing_binary_is_not_replaced_and_other_binaries_are_not_promoted(self):
        examples = self.consumer / EXAMPLES
        examples.mkdir(parents=True)
        old = examples / binary_names(self.shard)[-1]
        old.write_bytes(b'prior executable')
        with self.assertRaisesRegex(AssertionError, 'already present'):
            restore(self.consumer, self.identity, self.shard)
        self.assertEqual(list(examples.iterdir()), [old])
        self.assertEqual(old.read_bytes(), b'prior executable')

    def test_package_runtime_reuses_exact_playback_and_library_probes(self):
        shard = 'package-runtime'
        shared = self.producer / ARTIFACT
        target = self.consumer / ARTIFACT / shard
        (shared / shard).rename(target)
        restore(self.consumer, self.identity, shard)
        for original, name in (('playback', 'native_audio_probe.exe'), ('library-local', 'local_ui_probe.exe')):
            original_dir = self.staged if original == self.shard else shared / original
            self.assertEqual((self.consumer / EXAMPLES / name).read_bytes(),
                             (original_dir / name).read_bytes())
        self.assertEqual((self.consumer / EXAMPLES / COLD_FLAC_TEST).read_bytes(),
                         COLD_FLAC_TEST.encode())
        self.assertFalse(any(COLD_FLAC_TEST in binary_names(name) for name in PROBES if name != shard))

    def test_tampered_cold_flac_harness_prevents_promotion_of_the_entire_shard(self):
        shard = 'package-runtime'
        target = self.consumer / ARTIFACT / shard
        (self.producer / ARTIFACT / shard).rename(target)
        harness = target / COLD_FLAC_TEST
        harness.write_bytes(b'!' + harness.read_bytes()[1:])
        with self.assertRaisesRegex(AssertionError, 'provenance or checksum'):
            restore(self.consumer, self.identity, shard)
        self.assertFalse((self.consumer / EXAMPLES).exists())

    def test_workflow_uses_the_closed_shard_inventory(self):
        workflow = (Path(__file__).resolve().parents[2] / '.github/workflows/ci.yml').read_text()
        sections, shards = {}, []
        for job_id, dependency, excludes_package in (
            ('windows-webview', 'windows-release-runtime', True),
            ('windows-webview-packaged', 'package-windows', False),
        ):
            job = workflow.split(f'  {job_id}:\n', 1)[1]
            job = re.split(r'^  \w[\w-]*:\s*$', job, maxsplit=1, flags=re.MULTILINE)[0]
            self.assertIn(f'needs: [{dependency}, windows-webview-probes]', job)
            self.assertEqual('name: packages-windows-x64' not in job, excludes_package)
            for shard, section in re.findall(
                r'^      - name: Windows WebView regression \(([^)]+)\)\n'
                r'(.*?)(?=^      - name:|\Z)', job, flags=re.MULTILINE | re.DOTALL,
            ):
                self.assertNotIn(shard, sections)
                sections[shard] = section
            matrix = re.search(r'^        shard: \[([^\]]+)\]$', job, re.MULTILINE)
            self.assertIsNotNone(matrix)
            shards.extend(matrix[1].split(', '))
        self.assertEqual(set(sections), set(PROBES))
        self.assertEqual(Counter(shards), Counter(PROBES.keys()))
        producers = re.findall(r'^          name: windows-webview-probes-([\w-]+)-attempt-', workflow,
                               flags=re.MULTILINE)
        self.assertEqual(Counter(producers), Counter(PROBES.keys()))
        for shard, section in sections.items():
            expected = set(binary_names(shard))
            if shard not in {'catalog', 'package-runtime'}:
                expected.add(UX.name)
            self.assertEqual(set(re.findall(r'examples/([a-z0-9_]+\.exe)', section)),
                             expected)
            self.assertNotIn('cargo build', section)
