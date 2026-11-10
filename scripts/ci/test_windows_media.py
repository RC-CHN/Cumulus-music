"""Runtime cache/source integrity, using synthetic archives and recipes only."""
import hashlib
import json
import os
import shutil
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import contextlib
import io

from windows_media import verify_cache, native_pcm_report
from windows_media_recipe import (
    HERE, PACKAGE, RECIPE_INPUTS, build_package, configuration, download, patch_recipe, prepare, sha256,
)

RECIPE = '''pkgrel=3
depends=("${MINGW_PACKAGE_PREFIX}-aom")
makedepends=("${MINGW_PACKAGE_PREFIX}-cc")
source=(https://example.invalid/${_realname}-${pkgver}.tar.xz{,.asc})
sha256sums=('synthetic'
            'SKIP'
)
prepare() {
  mv VERSION VERSION.txt
}
build() {
  common_config+=(
    --enable-gpl
    --enable-libopus
    --enable-libvorbis
    --enable-libmp3lame
  )
  for _variant in -static -shared; do :; done
}
check() {
  for _variant in -static -shared; do make check || true; done
}

package() {
  for _variant in -static -shared; do :; done
}
'''


class WindowsSourceFallback(unittest.TestCase):
    def test_network_timeout_uses_mirror_with_exact_same_digest(self):
        primary = 'https://primary.invalid/source.tar.xz'
        mirror = 'https://mirror.invalid/source.tar.xz'
        source = b'synthetic locked archive'
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()), \
                patch('windows_media_recipe.time.sleep'), patch('windows_media_recipe.urllib.request.urlopen') as open_url:
            target = Path(directory) / 'source.tar.xz'
            open_url.side_effect = [urllib.error.URLError(TimeoutError('synthetic TLS timeout')), io.BytesIO(source)]
            download(primary, target, hashlib.sha256(source).hexdigest(), mirror=mirror)
            self.assertEqual([call.args[0] for call in open_url.call_args_list], [primary, mirror])
            self.assertEqual(target.read_bytes(), source)
            self.assertEqual(list(Path(directory).iterdir()), [target])

    def test_mirror_bytes_with_wrong_digest_are_not_published_or_retried(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()), \
                patch('windows_media_recipe.time.sleep'), patch('windows_media_recipe.urllib.request.urlopen') as open_url:
            target = Path(directory) / 'source.tar.xz'
            open_url.side_effect = [TimeoutError('synthetic TLS timeout'), io.BytesIO(b'wrong source')]
            with self.assertRaisesRegex(ValueError, 'source checksum mismatch'):
                download('https://primary.invalid/source.tar.xz', target,
                         hashlib.sha256(b'locked source').hexdigest(), mirror='https://mirror.invalid/source.tar.xz')
            self.assertEqual(open_url.call_count, 2)
            self.assertEqual(list(Path(directory).iterdir()), [])


class WindowsRecipeCachePlan(unittest.TestCase):
    def test_prepare_reuses_dependency_key_after_probe_only_edits(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root = Path(directory)
            for name in RECIPE_INPUTS:
                shutil.copyfile(HERE / name, root / name)
            version = json.loads((root / 'windows_media.lock.json').read_text())['upstreamPackage']
            installed = f'{PACKAGE} {version}\ntoolchain 1\n'
            baseline = 'ffmpeg synthetic\nconfiguration: --enable-shared --enable-gpl\n'
            build = root / 'out'
            github_output = root / 'github-output'

            def key(inventory=installed, config=baseline):
                github_output.unlink(missing_ok=True)
                with patch('windows_media_recipe.HERE', root), patch('windows_media_recipe.BUILD', build), \
                        patch('windows_media_recipe.output', side_effect=[inventory, config]), \
                        patch.dict(os.environ, GITHUB_OUTPUT=str(github_output)):
                    prepare(root)
                plan = json.loads((build / 'plan.json').read_text())
                self.assertEqual(plan['packages'], inventory)
                self.assertEqual(plan['configuration'], configuration(config))
                self.assertEqual(github_output.read_text().strip(), 'cache-key=' + plan['key'])
                return plan['key']

            installer = root / 'windows_media.py'
            installer.write_bytes(b'debug PCM verification')
            before = key()
            installer.write_bytes(b'release PCM verification with another player')
            self.assertEqual(key(), before)
            self.assertNotEqual(key(installed.replace('toolchain 1', 'toolchain 2')), before)
            self.assertNotEqual(key(config=baseline.replace('--enable-gpl', '--disable-gpl')), before)
            for name in RECIPE_INPUTS:
                with self.subTest(recipe_input=name):
                    path = root / name
                    original = path.read_bytes()
                    path.write_bytes(original + b'\n')
                    try:
                        self.assertNotEqual(key(), before)
                    finally:
                        path.write_bytes(original)


class WindowsRuntimeIntegrity(unittest.TestCase):
    def test_zero_exit_without_a_json_report_cannot_become_a_runtime_receipt(self):
        with tempfile.TemporaryDirectory() as directory, contextlib.redirect_stdout(io.StringIO()):
            root=Path(directory);stderr=root/'stderr.log';stderr.write_text('synthetic native stage')
            for text in ['', 'native banner\n{"ok":true,"remote_requests":0}', '{"ok":false,"remote_requests":0}', '{"ok":true,"remote_requests":false}']:
                with self.assertRaises((ValueError, AssertionError)):
                    native_pcm_report(text,root/'stdout.log',stderr)
                self.assertEqual((root/'stdout.log').read_text(),text)

    def test_valid_native_report_is_retained_without_discarding_any_output(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);text=' {"ok":true,"remote_requests":0,"pcm_cases":["合成"]}\n'
            value=native_pcm_report(text,root/'stdout.log',root/'stderr.log')
            self.assertTrue(value['ok']);self.assertEqual((root/'stdout.log').read_bytes(),text.encode())

    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.cache = Path(directory.name)
        self.lock = {'recipeCommit': 'synthetic-locked-recipe', 'recipeFiles': {'PKGBUILD': hashlib.sha256(RECIPE.encode()).hexdigest()},
                     'source': {'name': 'ffmpeg-synthetic.tar.xz', 'sha256': hashlib.sha256(b'source').hexdigest()}}
        self.plan = {'key': 'synthetic-dependency-identity', 'packages': 'synthetic-build-dependencies 1\n'}
        (self.cache / 'PKGBUILD.upstream').write_text(RECIPE, encoding='utf-8')
        (self.cache / 'PKGBUILD').write_text(patch_recipe(RECIPE), encoding='utf-8')
        (self.cache / 'ffmpeg-synthetic.tar.xz').write_bytes(b'source')
        self.package = self.cache / 'ffmpeg-synthetic.pkg.tar.zst'
        self.package.write_bytes(b'package')
        (self.cache / 'ffmpeg-ladspa-windows.patch').write_bytes((HERE / 'ffmpeg-ladspa-windows.patch').read_bytes())
        self.record()

    def record(self, **metadata):
        files = {p.name: sha256(p) for p in self.cache.iterdir() if p.name != 'build.json'}
        value = {'key': self.plan['key'], 'packages': self.plan['packages'],
                 'recipeCommit': self.lock['recipeCommit'], 'source': self.lock['source'], 'files': files}
        value.update(metadata)
        (self.cache / 'build.json').write_text(json.dumps(value), encoding='utf-8')

    def test_cold_builder_receipt_matches_the_same_strict_cache_verifier(self):
        build = self.cache / 'cold'
        build.mkdir()
        cache = build / 'cache'
        inputs = {'PKGBUILD': RECIPE.encode(), self.lock['source']['name']: b'source'}
        self.lock['source']['url'] = 'https://example.invalid/synthetic-source.tar.xz'

        def download_pinned(_url, target, expected, **_):
            content = inputs[target.name]
            self.assertEqual(hashlib.sha256(content).hexdigest(), expected)
            target.write_bytes(content)

        def build_synthetic(command, **_):
            self.assertEqual(command[1:3], ['--login', '/synthetic-makepkg-script'])
            self.assertEqual(Path(command[3]), cache)
            self.assertEqual((cache / 'PKGBUILD').read_text(), patch_recipe(RECIPE))
            (cache / 'ffmpeg-synthetic.pkg.tar.zst').write_bytes(b'isolated synthetic package')
            (cache / 'src').mkdir()

        with patch('windows_media_recipe.BUILD', build), \
                patch('windows_media_recipe.download', side_effect=download_pinned), \
                patch('windows_media_recipe.output', return_value='/synthetic-makepkg-script\n'), \
                patch('windows_media_recipe.subprocess.run', side_effect=build_synthetic), \
                contextlib.redirect_stdout(io.StringIO()):
            build_package(self.cache, cache, self.plan, self.lock)
        self.assertFalse((cache / 'src').exists())
        self.assertEqual(verify_cache(cache, self.plan, self.lock).read_bytes(), b'isolated synthetic package')

    def test_intact_source_and_package_are_accepted_without_executing_them(self):
        self.assertEqual(verify_cache(self.cache, self.plan, self.lock), self.package)

    def test_corrupted_package_is_rejected_before_installation(self):
        self.package.write_bytes(b'corrupted')
        with self.assertRaisesRegex(AssertionError, 'cache checksum mismatch'):
            verify_cache(self.cache, self.plan, self.lock)

    def test_dependency_identity_mismatch_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, 'identity mismatch'):
            verify_cache(self.cache, {'key': 'another-compiler'}, self.lock)

    def test_exact_key_does_not_override_inconsistent_receipt_identity(self):
        for metadata, message in (
            ({'packages': 'unreviewed-toolchain 2'}, 'package inventory mismatch'),
            ({'recipeCommit': 'different-upstream'}, 'recipe or source identity mismatch'),
            ({'source': {'name': 'unlocked-source'}}, 'recipe or source identity mismatch'),
        ):
            with self.subTest(metadata=metadata):
                self.record(**metadata)
                with self.assertRaisesRegex(AssertionError, message):
                    verify_cache(self.cache, self.plan, self.lock)

    def test_source_cannot_be_omitted_from_a_rehashed_cache(self):
        (self.cache / 'ffmpeg-synthetic.tar.xz').unlink()
        self.record()
        with self.assertRaisesRegex(AssertionError, 'lacks the FFmpeg source'):
            verify_cache(self.cache, self.plan, self.lock)

    def test_rehashed_source_corruption_still_fails_the_committed_lock(self):
        (self.cache / 'ffmpeg-synthetic.tar.xz').write_bytes(b'different source')
        self.record()
        with self.assertRaisesRegex(AssertionError, 'locked source checksum mismatch'):
            verify_cache(self.cache, self.plan, self.lock)

    def test_rehashed_recipe_cannot_remove_an_existing_codec(self):
        recipe = self.cache / 'PKGBUILD'
        recipe.write_text(recipe.read_text(encoding='utf-8').replace('--enable-libopus', '--disable-libopus'), encoding='utf-8')
        self.record()
        with self.assertRaisesRegex(AssertionError, 'differs from reviewed patch'):
            verify_cache(self.cache, self.plan, self.lock)

    def test_upstream_recipe_drift_is_an_explicit_failure(self):
        with self.assertRaisesRegex(AssertionError, 'upstream recipe changed'):
            patch_recipe(RECIPE.replace('pkgrel=3\n', 'pkgrel=4\n'))

    def test_rehashed_loader_patch_drift_is_rejected(self):
        (self.cache / 'ffmpeg-ladspa-windows.patch').write_bytes(b'unreviewed loader')
        self.record()
        with self.assertRaisesRegex(AssertionError, 'loader patch differs'):
            verify_cache(self.cache, self.plan, self.lock)

    def test_configuration_parser_preserves_quoted_compiler_arguments(self):
        self.assertEqual(configuration("ffmpeg synthetic\nconfiguration: --enable-shared --extra-cflags='-O2 -DTEST'\n"),
                         ['--enable-shared', '--extra-cflags=-O2 -DTEST'])


if __name__ == '__main__':
    unittest.main()
