"""Runtime cache/source integrity, using synthetic archives and recipes only."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
import contextlib
import io

from windows_media import HERE, configuration, patch_recipe, sha256, verify_cache, native_pcm_report

RECIPE = '''pkgrel=2
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
        self.lock = {'recipeFiles': {'PKGBUILD': hashlib.sha256(RECIPE.encode()).hexdigest()},
                     'source': {'name': 'ffmpeg-synthetic.tar.xz', 'sha256': hashlib.sha256(b'source').hexdigest()}}
        self.plan = {'key': 'synthetic-dependency-identity'}
        (self.cache / 'PKGBUILD.upstream').write_text(RECIPE, encoding='utf-8')
        (self.cache / 'PKGBUILD').write_text(patch_recipe(RECIPE), encoding='utf-8')
        (self.cache / 'ffmpeg-synthetic.tar.xz').write_bytes(b'source')
        self.package = self.cache / 'ffmpeg-synthetic.pkg.tar.zst'
        self.package.write_bytes(b'package')
        (self.cache / 'ffmpeg-ladspa-windows.patch').write_bytes((HERE / 'ffmpeg-ladspa-windows.patch').read_bytes())
        self.record()

    def record(self):
        files = {p.name: sha256(p) for p in self.cache.iterdir() if p.name != 'build.json'}
        (self.cache / 'build.json').write_text(json.dumps({'key': self.plan['key'], 'files': files}), encoding='utf-8')

    def test_intact_source_and_package_are_accepted_without_executing_them(self):
        self.assertEqual(verify_cache(self.cache, self.plan, self.lock), self.package)

    def test_corrupted_package_is_rejected_before_installation(self):
        self.package.write_bytes(b'corrupted')
        with self.assertRaisesRegex(AssertionError, 'cache checksum mismatch'):
            verify_cache(self.cache, self.plan, self.lock)

    def test_dependency_identity_mismatch_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, 'identity mismatch'):
            verify_cache(self.cache, {'key': 'another-compiler'}, self.lock)

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
            patch_recipe(RECIPE.replace('pkgrel=2\n', 'pkgrel=3\n'))

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
