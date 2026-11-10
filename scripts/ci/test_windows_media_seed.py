"""Only an exact attested legacy cache can become a current FFmpeg seed."""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from test_windows_media import RECIPE
import windows_media as media
import windows_media_recipe as recipe
import windows_media_seed as seed


class ReviewedPlan(unittest.TestCase):
    def test_exact_current_source_inventory_and_args_only(self):
        # Independent fixture of the archived v1 concatenation, not a rewrite
        # of the v2 key: each exact input is checked before legacy lookup.
        inventory = 'ffmpeg 9.0.2-3\ntoolchain 1\n'
        config = ['--enable-gpl', '--enable-shared']
        patched = ['--enable-gpl', '--enable-ladspa', '--enable-shared']
        historical = b'reviewed historical windows_media.py'
        inputs = [(recipe.HERE / name).read_bytes() for name in recipe.RECIPE_INPUTS]
        static = b''.join(len(data).to_bytes(8, 'big') + data for data in inputs)
        _, lock, build, loader = inputs
        expected = 'windows-media-ucrt64-' + hashlib.sha256(
            lock + historical + build + loader + inventory.encode()).hexdigest()
        plan = dict(key=recipe.cache_key(inventory, config), packages=inventory, configuration=config)
        with patch.object(seed, 'REVIEWED_RECIPE', hashlib.sha256(static).hexdigest()), \
                patch.object(seed, 'LEGACY_BLOB_SHA256', hashlib.sha256(historical).hexdigest()), \
                patch.object(seed, 'INVENTORY_SHA256', hashlib.sha256(inventory.encode()).hexdigest()), \
                patch.object(seed, 'PATCHED_CONFIG_SHA256', hashlib.sha256(json.dumps(
                    patched, separators=(',', ':')).encode()).hexdigest()), \
                patch.object(seed, 'LEGACY_CACHE_KEY', expected), \
                patch.object(seed, 'source_blob', return_value=historical) as blob:
            self.assertEqual(seed.legacy_key(plan), expected)
            blob.reset_mock()
            self.assertIsNone(seed.legacy_key({**plan, 'packages': inventory.replace('toolchain 1', 'toolchain 2')}))
            self.assertIsNone(seed.legacy_key({**plan, 'configuration': config[::-1]}))
            self.assertIsNone(seed.legacy_key({**plan, 'configuration': config + ['--enable-ladspa']}))
            self.assertIsNone(seed.legacy_key({**plan, 'key': 'unexpected-current-recipe'}))
            blob.assert_not_called()
            with patch.object(seed, 'REVIEWED_RECIPE', 'different-source'):
                self.assertIsNone(seed.legacy_key(plan))
            for source in (None, b'unreviewed blob'):
                blob.return_value = source
                self.assertIsNone(seed.legacy_key(plan))


class LegacyMigration(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.cache = self.root / 'cache'
        self.source_dir = self.root / 'source'
        self.source_dir.mkdir()
        shutil.copyfile(recipe.HERE / 'ffmpeg-ladspa-windows.patch',
                        self.source_dir / 'ffmpeg-ladspa-windows.patch')
        self.lock = {
            'recipeCommit': 'synthetic-reviewed-upstream', 'customPackage': 'synthetic-9.0.2-3.2',
            'recipeFiles': {'PKGBUILD': hashlib.sha256(RECIPE.encode()).hexdigest()},
            'source': {'name': 'synthetic-source.tar.xz', 'sha256': hashlib.sha256(b'source').hexdigest()},
        }
        self.plan = {
            'key': 'reviewed-current-key', 'packages': 'synthetic-toolchain 1\n',
            'configuration': ['--enable-gpl', '--enable-shared'],
        }
        self.legacy = 'reviewed-exact-legacy-key'
        self.package_name = 'synthetic-ffmpeg.pkg.tar.zst'
        self.attested = b'independently attested warm archive'
        self.make_cache(self.cache, self.legacy, self.attested)
        (self.root / 'plan.json').write_text(json.dumps(self.plan))
        (self.source_dir / 'windows_media.lock.json').write_text(json.dumps(self.lock))
        self.msys = self.root / 'msys'
        binary = self.msys / 'ucrt64/bin'
        binary.mkdir(parents=True)
        for lib in ('avcodec', 'avdevice', 'avfilter', 'avformat', 'avutil', 'swresample', 'swscale'):
            (binary / f'{lib}-synthetic.dll').write_bytes(lib.encode())
        self.target = self.root / 'target'
        for profile in ('debug', 'release'):
            output = self.target / profile
            (output / 'examples').mkdir(parents=True)
            (output / 'examples/player_probe.exe').write_bytes(profile.encode())
            (output / 'cumulus_audio_filter.dll').write_bytes(profile.encode())
        stack = self.enterContext(contextlib.ExitStack())
        stack.enter_context(patch.object(media, 'BUILD', self.root))
        stack.enter_context(patch.object(media, 'HERE', self.source_dir))
        stack.enter_context(patch.object(seed, 'legacy_key', return_value=self.legacy))
        stack.enter_context(patch.object(seed, 'PACKAGE_NAME', self.package_name))
        stack.enter_context(patch.object(seed, 'PACKAGE_SHA256', hashlib.sha256(self.attested).hexdigest()))
        stack.enter_context(patch.object(seed, 'RECEIPT_SHA256', recipe.sha256(self.cache / 'build.json')))
        stack.enter_context(patch.dict(os.environ, CARGO_TARGET_DIR=str(self.target), GITHUB_ACTIONS='true'))
        stack.enter_context(contextlib.redirect_stdout(io.StringIO()))

    def make_cache(self, path, key, package):
        path.mkdir(exist_ok=True)
        (path / 'PKGBUILD.upstream').write_text(RECIPE, encoding='utf-8', newline='\n')
        (path / 'PKGBUILD').write_text(recipe.patch_recipe(RECIPE), encoding='utf-8', newline='\n')
        shutil.copyfile(self.source_dir / 'ffmpeg-ladspa-windows.patch', path / 'ffmpeg-ladspa-windows.patch')
        (path / self.lock['source']['name']).write_bytes(b'source')
        (path / self.package_name).write_bytes(package)
        files = {file.name: recipe.sha256(file) for file in path.iterdir() if file.name != 'build.json'}
        receipt = {'key': key, 'recipeCommit': self.lock['recipeCommit'], 'source': self.lock['source'],
                   'packages': self.plan['packages'], 'files': files}
        (path / 'build.json').write_text(json.dumps(receipt))

    def assert_cold(self, matched):
        def build_current(_msys, cache, plan, _lock):
            self.assertFalse(cache.exists(), 'untrusted restore must be moved before source build')
            self.make_cache(cache, plan['key'], b'fresh source build')

        with patch.object(media, 'build_package', side_effect=build_current) as build:
            package, legacy = media.select_package(self.msys, self.cache, self.plan, self.lock, legacy_match=matched)
        build.assert_called_once()
        self.assertIsNone(legacy)
        self.assertEqual(package.read_bytes(), b'fresh source build')
        self.assertEqual(len(list(self.root.glob('rejected-cache-*'))), 1)

    def test_prefix_hit_is_rejected_before_cache_verification_and_cold_builds(self):
        with patch.object(media, 'verify_cache', wraps=media.verify_cache) as verify:
            self.assert_cold(self.legacy + '-suffix')
        verify.assert_called_once_with(self.cache, self.plan, self.lock)

    def test_archive_with_forged_self_receipt_cannot_seed_and_cold_builds(self):
        self.make_cache(self.cache, self.legacy, b'corrupt package with a matching self-receipt')
        self.assert_cold(self.legacy)

    def test_unlisted_file_cannot_travel_with_verified_source_provenance(self):
        (self.cache / 'unreviewed-file').write_bytes(b'extra file outside the attested package')
        self.assert_cold(self.legacy)

    def run_release_install(self, config):
        def output(command, **_):
            if '-u' in command:
                return str(command[-1])
            if '-Q' in command:
                return 'ffmpeg ' + self.lock['customPackage']
            if '-version' in command:
                return 'configuration: ' + ' '.join(config)
            if '-filters' in command:
                return ' ladspa '
            raise AssertionError(command)

        with patch.object(media, 'output', side_effect=output), \
                patch.object(media.subprocess, 'run', return_value=subprocess.CompletedProcess(
                    [], 0, stdout='{"ok":true,"remote_requests":0}')) as run:
            media.install(self.msys, profile='release', legacy_match=self.legacy)
        return [call.args[0] for call in run.call_args_list]

    def test_old_archive_remains_legacy_until_current_pcm_and_release_proof(self):
        with self.assertRaises(FileNotFoundError):
            media.promote_seed('release', self.legacy)
        calls = self.run_release_install(['--enable-gpl', '--enable-ladspa', '--enable-shared'])
        self.assertEqual(len(calls), 3)
        self.assertIn('-U', calls[0])
        self.assertIn('ladspa=file=./cumulus_audio_filter.dll:plugin=cumulus_geq:controls=1', calls[1])
        self.assertEqual(Path(calls[2][0]), self.target / 'release/examples/player_probe.exe')
        self.assertEqual(Path(calls[2][1]), self.target / 'release/cumulus_audio_filter.dll')
        self.assertEqual(json.loads((self.cache / 'build.json').read_text())['key'], self.legacy)
        with self.assertRaisesRegex(AssertionError, 'stale media seed proof'):
            media.promote_seed('release', self.legacy + '-suffix')
        self.assertEqual(json.loads((self.cache / 'build.json').read_text())['key'], self.legacy)
        media.promote_seed('release', self.legacy)
        saved = json.loads((self.cache / 'build.json').read_text())
        self.assertEqual(saved['key'], self.plan['key'])
        self.assertEqual(saved['seededFrom'], self.legacy)
        self.assertEqual(media.verify_cache(self.cache, self.plan, self.lock).read_bytes(), self.attested)

    def test_installed_config_superset_is_not_a_migratable_identity(self):
        with self.assertRaisesRegex(AssertionError, 'seeded media configuration is not exact'):
            self.run_release_install(['--enable-gpl', '--enable-ladspa', '--enable-shared', '--enable-extra'])
        self.assertFalse((self.root / 'installed.json').exists())
        self.assertEqual(json.loads((self.cache / 'build.json').read_text())['key'], self.legacy)


if __name__ == '__main__':
    unittest.main()
