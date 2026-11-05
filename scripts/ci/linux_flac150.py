"""Build a non-shipping, hash-pinned FLAC 1.5.0 runtime for sealed Linux proofs."""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path, PurePosixPath
import platform
import shutil
import subprocess
import tarfile
import time
import urllib.request

URL = 'https://downloads.xiph.org/releases/flac/flac-1.5.0.tar.xz'
ARCHIVE_SHA256 = 'f2c1c76592a82ffff8413ba3c4a1299b6c7ab06c734dee03fd88630485c2b920'
VERSION = '1.5.0'
MAX_ARCHIVE = 4 * 1024 * 1024
MAX_SOURCE = 48 * 1024 * 1024
MAX_MEMBERS = 4096
OPTIONS = (
    '-DCMAKE_BUILD_TYPE=Release', '-DBUILD_SHARED_LIBS=ON', '-DWITH_OGG=OFF',
    '-DBUILD_CXXLIBS=OFF', '-DBUILD_PROGRAMS=OFF', '-DBUILD_TESTING=OFF',
    '-DBUILD_EXAMPLES=OFF', '-DBUILD_DOCS=OFF', '-DINSTALL_MANPAGES=OFF',
)


def sha256(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


class HttpsRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        if not newurl.startswith('https://'):
            raise ValueError('FLAC archive redirect must use HTTPS')
        return super().redirect_request(request, fp, code, msg, headers, newurl)


def download(path):
    deadline = time.monotonic() + 90
    opener = urllib.request.build_opener(HttpsRedirect())
    with opener.open(URL, timeout=30) as response, path.open('xb') as output:
        if not response.geturl().startswith('https://'):
            raise ValueError('FLAC archive must use HTTPS')
        declared = response.headers.get('Content-Length')
        if declared is not None and int(declared) > MAX_ARCHIVE:
            raise ValueError('FLAC archive exceeds download limit')
        total = 0
        while chunk := response.read(65536):
            total += len(chunk)
            if total > MAX_ARCHIVE or time.monotonic() >= deadline:
                raise ValueError('FLAC archive exceeds bounded download')
            output.write(chunk)
    if sha256(path) != ARCHIVE_SHA256:
        raise ValueError('official FLAC archive SHA256 mismatch')


def unpack(archive, destination):
    """Copy bounded regular members only; never honor links, owners or modes."""
    destination.mkdir()
    total = 0
    seen = set()
    with tarfile.open(archive, 'r:xz') as source:
        for index, member in enumerate(source):
            if index >= MAX_MEMBERS:
                raise ValueError('FLAC archive member limit')
            path = PurePosixPath(member.name)
            if (path.is_absolute() or '..' in path.parts or not path.parts
                    or path.parts[0] != 'flac-1.5.0' or '\\' in member.name
                    or member.name in seen or not (member.isdir() or member.isfile())):
                raise ValueError('unsafe FLAC archive member')
            seen.add(member.name)
            if member.size < 0 or member.size > 8 * 1024 * 1024:
                raise ValueError('FLAC archive file limit')
            total += member.size
            if total > MAX_SOURCE:
                raise ValueError('FLAC archive source limit')
            target = destination.joinpath(*path.parts)
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as contents, target.open('xb') as out:
                    shutil.copyfileobj(contents, out, length=65536)
    root = destination / 'flac-1.5.0'
    if not (root / 'CMakeLists.txt').is_file():
        raise ValueError('official FLAC source root missing')
    return root


def runtime_version(library):
    raw = library.read_bytes()
    if raw[:6] != b'\x7fELF\x02\x01' or raw[18:20] != b'\x3e\x00':
        raise ValueError('FLAC proof runtime must be Linux x86_64 ELF')
    loaded = ctypes.CDLL(str(library))
    version = ctypes.c_char_p.in_dll(loaded, 'FLAC__VERSION_STRING').value
    if version != b'1.5.0':
        raise ValueError('native FLAC proof version must be exactly 1.5.0')
    return version.decode('ascii')


def build(root):
    if platform.system() != 'Linux' or platform.machine() not in ['x86_64', 'AMD64']:
        raise ValueError('this proof-only build requires Linux x86_64')
    root.mkdir(parents=True, exist_ok=False)
    report = {'ok': False, 'sourceUrl': URL, 'sourceArchiveSha256': ARCHIVE_SHA256,
              'version': VERSION, 'privateRuntime': True, 'shippingRuntimeModified': False,
              'globalInstallPerformed': False, 'cmakeOptions': list(OPTIONS)}
    try:
        archive = root / 'flac-1.5.0.tar.xz'
        download(archive)
        source = unpack(archive, root / 'source')
        build_dir = root / 'cmake-build'
        with (root / 'build.log').open('x', encoding='utf-8') as log:
            for command in [
                ['cmake', '-S', str(source), '-B', str(build_dir), *OPTIONS],
                ['cmake', '--build', str(build_dir), '--target', 'FLAC', '--parallel', '2'],
            ]:
                subprocess.run(command, stdout=log, stderr=subprocess.STDOUT,
                               check=True, timeout=300)
        candidates = list((build_dir / 'src/libFLAC').glob('libFLAC.so.*.*.*'))
        if len(candidates) != 1 or candidates[0].is_symlink():
            raise ValueError('expected exactly one real built FLAC runtime')
        runtime = root / 'runtime'
        runtime.mkdir()
        library = runtime / 'libFLAC.so.14'
        shutil.copyfile(candidates[0], library)
        report.update(library=str(library.resolve(strict=True)),
                      librarySha256=sha256(library),
                      loadedVersion=runtime_version(library),
                      cmakeVersion=subprocess.check_output(['cmake', '--version'], text=True).splitlines()[0])
        report['ok'] = True
        return library
    except Exception as error:
        report['failure'] = str(error)
        raise
    finally:
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-dir', type=Path, required=True, help='new non-shipping build directory')
    args = parser.parse_args()
    print(build(args.build_dir.resolve()))


if __name__ == '__main__':
    main()
