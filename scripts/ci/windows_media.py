"""Build the pinned, complete UCRT64 FFmpeg recipe with LADSPA support.

No codecs are removed. Only the shared runtime variant is distributed. Cache
identity covers every installed dependency plus the recipe and build scripts;
cached packages are authenticated and the installed filter is checked every run.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
BUILD = ROOT / '.build/windows-media'
PACKAGE = 'mingw-w64-ucrt-x86_64-ffmpeg'


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def configuration(version):
    line = next(line for line in version.splitlines() if line.lstrip().startswith('configuration:'))
    return shlex.split(line.split('configuration:', 1)[1])


def patch_recipe(original):
    """Fail closed when the locked upstream recipe no longer has this shape."""
    replacements = [
        ('pkgrel=2\n', 'pkgrel=2.2\n'),
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


def native_pcm_report(stdout, destination, stderr):
    """Keep the full response even when exit=0 has no valid verification report."""
    destination.write_text(stdout, encoding='utf-8', newline='')
    try:
        report = json.loads(stdout)
        assert isinstance(report, dict) and report.get('ok') is True
        assert type(report.get('remote_requests')) is int and report['remote_requests'] == 0
    except (ValueError, AssertionError):
        print('Native PCM verifier produced no valid success report; exit=0 is insufficient.', flush=True)
        print(f'Captured stdout: {stdout[:4000]!r}', flush=True)
        if stderr.exists():
            print(stderr.read_text(encoding='utf-8', errors='replace')[-16000:], flush=True)
        raise
    return report


def prepare(msys):
    BUILD.mkdir(parents=True, exist_ok=True)
    lock = json.loads((HERE / 'windows_media.lock.json').read_text(encoding='utf-8'))
    installed = output([msys / 'usr/bin/pacman.exe', '-Q'])
    version = dict(line.split(' ', 1) for line in installed.splitlines())[PACKAGE]
    assert version == lock['upstreamPackage'], f'update/review the runtime lock for FFmpeg {version}'
    binary = msys / 'ucrt64/bin'
    baseline = output([binary / 'ffmpeg.exe', '-version'])
    identity = hashlib.sha256()
    for path in (HERE / 'windows_media.lock.json', HERE / 'windows_media.py',
                 HERE / 'windows_media_build.sh', HERE / 'ffmpeg-ladspa-windows.patch'):
        identity.update(path.read_bytes())
    identity.update(installed.encode())
    key = 'windows-media-ucrt64-' + identity.hexdigest()
    write_json(BUILD / 'plan.json', {'key': key, 'packages': installed, 'configuration': configuration(baseline)})
    with open(os.environ['GITHUB_OUTPUT'], 'a', encoding='utf-8') as stream:
        stream.write(f'cache-key={key}\n')
    print(f'Prepared {key}, baseline FFmpeg {version}', flush=True)


def download(url, path, expected):
    with urllib.request.urlopen(url, timeout=60) as response, path.open('xb') as destination:
        shutil.copyfileobj(response, destination)
    assert sha256(path) == expected, f'source checksum mismatch: {path.name}'


def verify_cache(cache, plan, lock=None):
    if lock is None:
        lock = json.loads((HERE / 'windows_media.lock.json').read_text(encoding='utf-8'))
    receipt = json.loads((cache / 'build.json').read_text(encoding='utf-8'))
    assert receipt['key'] == plan['key'], 'runtime cache identity mismatch'
    for name, expected in receipt['files'].items():
        assert Path(name).name == name, 'invalid runtime cache member'
        assert sha256(cache / name) == expected, f'runtime cache checksum mismatch: {name}'
    packages = list(cache.glob('*.pkg.tar.zst'))
    assert len(packages) == 1 and packages[0].name in receipt['files'], 'missing/ambiguous runtime package'
    for name, expected in lock['recipeFiles'].items():
        source_name = 'PKGBUILD.upstream' if name == 'PKGBUILD' else name
        assert source_name in receipt['files'], f'cache lacks corresponding source: {source_name}'
        assert sha256(cache / source_name) == expected, f'locked recipe checksum mismatch: {source_name}'
    source = lock['source']
    assert source['name'] in receipt['files'], 'cache lacks the FFmpeg source archive'
    assert sha256(cache / source['name']) == source['sha256'], 'locked source checksum mismatch'
    expected_recipe = patch_recipe((cache / 'PKGBUILD.upstream').read_text(encoding='utf-8'))
    assert (cache / 'PKGBUILD').read_text(encoding='utf-8') == expected_recipe, 'modified recipe differs from reviewed patch'
    patch = 'ffmpeg-ladspa-windows.patch'
    assert patch in receipt['files'] and sha256(cache / patch) == sha256(HERE / patch), 'Windows loader patch differs from reviewed source'
    return packages[0]


def install(msys):
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    lock = json.loads((HERE / 'windows_media.lock.json').read_text(encoding='utf-8'))
    cache = BUILD / 'cache'
    if not (cache / 'build.json').exists():
        cache.mkdir(exist_ok=False)
        source_url = ('https://raw.githubusercontent.com/msys2/MINGW-packages/'
                      f"{lock['recipeCommit']}/mingw-w64-ffmpeg/")
        for name, expected in lock['recipeFiles'].items():
            download(source_url + name, cache / name, expected)
        shutil.copy2(cache / 'PKGBUILD', cache / 'PKGBUILD.upstream')
        shutil.copy2(HERE / 'ffmpeg-ladspa-windows.patch', cache / 'ffmpeg-ladspa-windows.patch')
        (cache / 'PKGBUILD').write_text(patch_recipe((cache / 'PKGBUILD').read_text(encoding='utf-8')), encoding='utf-8', newline='\n')
        source = lock['source']
        download(source['url'], cache / source['name'], source['sha256'])
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
    package = verify_cache(cache, plan)
    package_path = output([msys / 'usr/bin/cygpath.exe', '-u', package]).strip()
    subprocess.run([str(msys / 'usr/bin/pacman.exe'), '-U', '--noconfirm', package_path], check=True)
    version = output([msys / 'usr/bin/pacman.exe', '-Q', PACKAGE]).strip().split(' ', 1)[1]
    assert version == lock['customPackage'], 'custom FFmpeg package was not installed'
    binary = msys / 'ucrt64/bin'
    current = configuration(output([binary / 'ffmpeg.exe', '-version']))
    missing = set(plan['configuration']) - set(current)
    assert not missing, f'FFmpeg configuration regressed: {sorted(missing)}'
    assert '--enable-ladspa' in current
    filters = output([binary / 'ffmpeg.exe', '-hide_banner', '-filters'])
    assert re.search(r'\bladspa\s+', filters), 'installed FFmpeg cannot load LADSPA'
    # The real current plugin must load, not merely appear in a capability list.
    target = Path(os.environ.get('CARGO_TARGET_DIR', ROOT / '.build/target'))
    plugin = target / 'debug/cumulus_audio_filter.dll'
    assert plugin.is_file(), 'build the current Cumulus DSP before runtime verification'
    shutil.copy2(plugin, BUILD / 'cumulus_audio_filter.dll')
    subprocess.run([str(binary / 'ffmpeg.exe'), '-nostdin', '-hide_banner', '-v', 'error',
                    '-f', 'lavfi', '-i', 'sine=frequency=1000:duration=0.1',
                    '-af', 'ladspa=file=./cumulus_audio_filter.dll:plugin=cumulus_geq:controls=1',
                    '-f', 'null', '-'], cwd=BUILD, check=True, timeout=30)
    # Check the production host's absolute, escaped Unicode path in-process:
    # loading the relative ASCII plugin above cannot establish this capability.
    probe = target / 'debug/examples/player_probe.exe'
    env = {**os.environ, 'PATH': str(binary) + os.pathsep + os.environ.get('PATH', '')}
    with (BUILD / 'player-probe.log').open('x', encoding='utf-8') as log:
        try:
            result = subprocess.run([str(probe), str(plugin.resolve()), str(BUILD / 'equalizer-proof')],
                                    stdout=subprocess.PIPE, stderr=log, env=env, check=True,
                                    text=True, encoding='utf-8', timeout=60)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            captured = error.stdout or ''
            if isinstance(captured, bytes):
                captured = captured.decode('utf-8', errors='replace')
            (BUILD / 'player-probe.stdout.log').write_text(captured, encoding='utf-8', newline='')
            log.flush()
            print((BUILD / 'player-probe.log').read_text(encoding='utf-8', errors='replace')[-16000:], flush=True)
            raise
    pcm = native_pcm_report(result.stdout, BUILD / 'player-probe.stdout.log', BUILD / 'player-probe.log')
    write_json(BUILD / 'player-probe.json', pcm)
    libraries = {p.name: sha256(p) for p in sorted(binary.glob('*.dll'))
                 if p.name.startswith(('avcodec-', 'avdevice-', 'avfilter-', 'avformat-', 'avutil-', 'swresample-', 'swscale-'))}
    assert len(libraries) == 7, f'FFmpeg library set changed: {sorted(libraries)}'
    receipt = {'key': plan['key'], 'package': version, 'configuration': current,
               'libraries': libraries, 'filterLoaded': True, 'nativePcmVerified': True,
               'pluginSha256': sha256(plugin)}
    write_json(BUILD / 'installed.json', receipt)
    print(json.dumps(receipt), flush=True)


def check_built_cache(_msys):
    """Authenticate build artifacts; this is not an installed-runtime proof."""
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    package = verify_cache(BUILD / 'cache', plan)
    print(f'Build artifact cache verified: {package.name}', flush=True)


def copy_provenance(destination, msys):
    """Require the verified runtime; retain its corresponding source in bundles."""
    receipt = json.loads((BUILD / 'installed.json').read_text(encoding='utf-8'))
    assert receipt['filterLoaded'] and receipt['nativePcmVerified'] and '--enable-ladspa' in receipt['configuration']
    for name, expected in receipt['libraries'].items():
        assert sha256(msys / 'ucrt64/bin' / name) == expected, f'FFmpeg changed after verification: {name}'
    target = destination / 'third-party/cumulus-ffmpeg-source'
    target.mkdir(parents=True)
    cache = BUILD / 'cache'
    plan = json.loads((BUILD / 'plan.json').read_text(encoding='utf-8'))
    verify_cache(cache, plan)
    for path in cache.iterdir():
        if path.is_file() and not path.name.endswith('.pkg.tar.zst'):
            shutil.copy2(path, target)
    shutil.copy2(BUILD / 'installed.json', target / 'installed.json')
    shutil.copy2(HERE / 'windows_media.lock.json', target / 'cumulus-lock.json')
    return receipt


if __name__ == '__main__':
    assert sys.platform == 'win32', 'the runtime is built and checked on Windows'
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['prepare', 'install', 'verify-cache'])
    parser.add_argument('--msys', type=Path, required=True)
    args = parser.parse_args()
    {'prepare': prepare, 'install': install, 'verify-cache': check_built_cache}[args.action](args.msys.resolve())
