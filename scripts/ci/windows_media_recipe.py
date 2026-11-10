"""Source recipe and cache identity for the shared UCRT64 FFmpeg package."""
import hashlib
import http.client
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
BUILD = ROOT / '.build/windows-media'
PACKAGE = 'mingw-w64-ucrt-x86_64-ffmpeg'
RECIPE_INPUTS = (
    'windows_media_recipe.py',
    'windows_media.lock.json',
    'windows_media_build.sh',
    'ffmpeg-ladspa-windows.patch',
)


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def configuration(version):
    line = next(line for line in version.splitlines() if line.lstrip().startswith('configuration:'))
    return shlex.split(line.split('configuration:', 1)[1])


def patch_recipe(original):
    """Fail closed when the locked upstream recipe no longer has this shape."""
    replacements = [
        ('pkgrel=3\n', 'pkgrel=3.2\n'),
        ('    --enable-gpl\n', '    --enable-gpl\n    --enable-ladspa\n'),
        ('depends=("${MINGW_PACKAGE_PREFIX}-aom"',
         'depends=("${MINGW_PACKAGE_PREFIX}-dlfcn"\n         "${MINGW_PACKAGE_PREFIX}-aom"'),
        ('makedepends=("${MINGW_PACKAGE_PREFIX}-cc"',
         'makedepends=("${MINGW_PACKAGE_PREFIX}-ladspa-sdk"\n             "${MINGW_PACKAGE_PREFIX}-cc"'),
        ('${_realname}-${pkgver}.tar.xz{,.asc}', '${_realname}-${pkgver}.tar.xz'),
        ("            'SKIP'\n", ''),
        ('  mv VERSION VERSION.txt\n',
         '  mv VERSION VERSION.txt\n  patch -Nbp1 -i "${startdir}/ffmpeg-ladspa-windows.patch"\n'),
    ]
    result = original
    for before, after in replacements:
        assert result.count(before) == 1, f'upstream recipe changed: {before!r}'
        result = result.replace(before, after)
    assert result.count('for _variant in -static -shared; do') == 3
    result = result.replace('for _variant in -static -shared; do', 'for _variant in -shared; do')
    start, end = result.index('check() {\n'), result.index('\npackage() {\n')
    # Unlike upstream's `make check || true`, a missing runtime feature fails.
    # Full current-DSP/PCM/GUI validation runs against the installed package.
    result = result[:start] + '''check() {
  cd "${srcdir}/build-${MSYSTEM}-shared"
  local runtime_path="$PWD/libavcodec:$PWD/libavdevice:$PWD/libavfilter:$PWD/libavformat:$PWD/libavutil:$PWD/libswresample:$PWD/libswscale:$PATH"
  PATH="$runtime_path" ./ffmpeg.exe -hide_banner -filters > cumulus-filters.txt
  grep -Eq '[[:space:]]ladspa[[:space:]]' cumulus-filters.txt
}
''' + result[end:]
    return result


def output(command, **kwargs):
    return subprocess.check_output(list(map(str, command)), text=True, encoding='utf-8', errors='strict', **kwargs)


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n', encoding='utf-8')


def cache_key(installed, configure_args):
    """Bind the complete recipe, locked inputs and exact MSYS2 build environment."""
    identity = hashlib.sha256()
    for name in RECIPE_INPUTS:
        value = (HERE / name).read_bytes()
        identity.update(len(value).to_bytes(8, 'big'))
        identity.update(value)
    identity.update(json.dumps([installed, configure_args], separators=(',', ':')).encode())
    return 'windows-media-ucrt64-recipe-v2-' + identity.hexdigest()


def prepare(msys):
    BUILD.mkdir(parents=True, exist_ok=True)
    lock = json.loads((HERE / 'windows_media.lock.json').read_text(encoding='utf-8'))
    installed = output([msys / 'usr/bin/pacman.exe', '-Q'])
    version = dict(line.split(' ', 1) for line in installed.splitlines())[PACKAGE]
    assert version == lock['upstreamPackage'], f'update/review the runtime lock for FFmpeg {version}'
    binary = msys / 'ucrt64/bin'
    baseline = output([binary / 'ffmpeg.exe', '-version'])
    configure_args = configuration(baseline)
    key = cache_key(installed, configure_args)
    write_json(BUILD / 'plan.json', {'key': key, 'packages': installed, 'configuration': configure_args})
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
        stream.write(f'cache-key={key}\n')
    print(f'Prepared {key}, baseline FFmpeg {version}', flush=True)


def download(url, path, expected, *, mirror=None):
    if path.exists():
        raise FileExistsError(path)
    sources = (url, mirror, url, mirror) if mirror else (url,) * 3
    for attempt, source_url in enumerate(sources):
        # Partial transfers never become recipe inputs. Each attempt owns only
        # its temporary file and retains the same pinned SHA-256 requirement.
        with tempfile.NamedTemporaryFile(dir=path.parent, prefix=path.name + '.', suffix='.part', delete=False) as stream:
            temporary = Path(stream.name)
        try:
            with urllib.request.urlopen(source_url, timeout=60) as response, temporary.open('wb') as destination:
                shutil.copyfileobj(response, destination)
            if sha256(temporary) != expected:
                raise ValueError(f'source checksum mismatch: {path.name}')
            temporary.rename(path)
            return
        except (urllib.error.URLError, TimeoutError, ConnectionError, http.client.IncompleteRead) as error:
            if isinstance(error, urllib.error.HTTPError) and error.code not in {429, 500, 502, 503, 504}:
                raise
            if attempt + 1 == len(sources):
                raise
            next_source = 'mirror' if mirror and sources[attempt + 1] == mirror else 'primary'
            print(f'Retrying source transfer {path.name} ({attempt + 2}/{len(sources)}, {next_source})', flush=True)
            time.sleep(2 ** attempt)
        finally:
            temporary.unlink(missing_ok=True)


def build_package(msys, cache, plan, lock):
    cache.mkdir(exist_ok=False)
    source_url = ('https://raw.githubusercontent.com/msys2/MINGW-packages/'
                  f"{lock['recipeCommit']}/mingw-w64-ffmpeg/")
    for name, expected in lock['recipeFiles'].items():
        download(source_url + name, cache / name, expected)
    shutil.copy2(cache / 'PKGBUILD', cache / 'PKGBUILD.upstream')
    shutil.copy2(HERE / 'ffmpeg-ladspa-windows.patch', cache / 'ffmpeg-ladspa-windows.patch')
    (cache / 'PKGBUILD').write_text(patch_recipe((cache / 'PKGBUILD').read_text(encoding='utf-8')), encoding='utf-8', newline='\n')
    source = lock['source']
    download(source['url'], cache / source['name'], source['sha256'], mirror=source.get('mirror'))
    env = {**os.environ, 'MSYSTEM': 'UCRT64', 'CHERE_INVOKING': '1', 'MINGW_ARCH': 'ucrt64'}
    script = output([msys / 'usr/bin/cygpath.exe', '-u', HERE / 'windows_media_build.sh']).strip()
    print('Building complete UCRT64 FFmpeg with LADSPA; progress is retained in .build/windows-media/build.log', flush=True)
    with (BUILD / 'build.log').open('x', encoding='utf-8') as log:
        try:
            subprocess.run([str(msys / 'usr/bin/bash.exe'), '--login', script, str(cache)],
                           env=env, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=5400)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            print((BUILD / 'build.log').read_text(encoding='utf-8', errors='replace')[-24000:], flush=True)
            raise
    # Keep the exact archive, original/modified recipes and hashes with the
    # package. The temporary source/object tree is not cached or shipped.
    if (cache / 'src').exists():
        shutil.rmtree(cache / 'src')
    files = {p.name: sha256(p) for p in sorted(cache.iterdir()) if p.is_file()}
    write_json(cache / 'build.json', {'key': plan['key'], 'recipeCommit': lock['recipeCommit'],
                                    'source': source, 'files': files, 'packages': plan['packages']})
