"""Stage the pinned UCRT64 libFLAC decoder and describe the actual PE closure."""
import hashlib
from pathlib import Path
import re
import subprocess
import tempfile

from windows_media import download, sha256

FLAC = 'libFLAC.dll'
FLAC_VERSION = '1.5.0'
COMPILE_DIGEST_ENV = 'CUMULUS_PACKAGED_WINDOWS_FLAC_SHA256'
FLAC_PACKAGE = 'mingw-w64-ucrt-x86_64-flac'
FLAC_RELEASE = '1.5.0-2'
FLAC_ARCHIVE = f'{FLAC_PACKAGE}-{FLAC_RELEASE}-any.pkg.tar.zst'
FLAC_URL = f'https://repo.msys2.org/mingw/ucrt64/{FLAC_ARCHIVE}'
FLAC_ARCHIVE_SHA256 = '28e8637795bd7d80289ddbb582d6ad350b73eac64390b877149717dd3757ad79'
FLAC_DLL_SHA256 = 'c01d2adbb5f3a09c27bb6a857553d2f2143293b6e07aab0ac0b2a203337e54eb'
FLAC_DLL_BYTES = 463251
FLAC_LICENSE = 'third-party/licenses/flac-1.5.0/COPYING.Xiph'
FLAC_LICENSE_SHA256 = '7866ee98760fc1f0156b4fe6bf530257e02be487ab3fd94e2b63799dd32d6b2c'
FLAC_LICENSE_BYTES = 1509
FLAC_IMPORTS = frozenset({'libogg-0.dll', 'libwinpthread-1.dll'})
PE_IMPORTS = FLAC_IMPORTS | {'kernel32.dll'} | {
    f'api-ms-win-crt-{part}-l1-1-0.dll'
    for part in ('convert', 'filesystem', 'heap', 'locale', 'math', 'private',
                 'runtime', 'stdio', 'string', 'time', 'utility')
}


def flac_imports(library, objdump):
    metadata = subprocess.check_output(
        [objdump, '-p', library], encoding='utf-8', errors='replace', timeout=20,
    )
    imports = {name.lower() for name in re.findall(r'DLL Name:\s*(\S+)', metadata)}
    if 'file format pei-x86-64' not in metadata or imports != PE_IMPORTS:
        raise ValueError('unexpected architecture or imports for the pinned libFLAC DLL')
    return imports


def stage_decoder(destination, msys):
    """Read only named, hashed members of the locked archive; never use ambient FLAC."""
    library = destination / FLAC
    license_file = destination / FLAC_LICENSE
    if any(item.name.lower() == FLAC.lower() for item in destination.iterdir()):
        raise ValueError('package stage already contains libFLAC')
    with tempfile.TemporaryDirectory(prefix='cumulus-windows-flac-') as folder:
        archive = Path(folder) / FLAC_ARCHIVE
        download(FLAC_URL, archive, FLAC_ARCHIVE_SHA256)
        msys_archive = subprocess.check_output(
            [msys / 'usr/bin/cygpath.exe', '-u', archive], encoding='utf-8', timeout=10,
        ).strip()

        def read(member, size, expected):
            data = subprocess.check_output(
                [msys / 'usr/bin/bsdtar.exe', '-xOf', msys_archive, member], timeout=20,
            )
            if len(data) != size or hashlib.sha256(data).hexdigest() != expected:
                raise ValueError(f'pinned MSYS2 archive member failed verification: {member}')
            return data

        binary = read(f'ucrt64/bin/{FLAC}', FLAC_DLL_BYTES, FLAC_DLL_SHA256)
        notice = read('ucrt64/share/licenses/flac/COPYING.Xiph', FLAC_LICENSE_BYTES, FLAC_LICENSE_SHA256)
        candidate = archive.parent / FLAC
        candidate.write_bytes(binary)
        imports = flac_imports(candidate, msys / 'ucrt64/bin/objdump.exe')
        license_file.parent.mkdir(parents=True, exist_ok=False)
        with license_file.open('xb') as output:
            output.write(notice)
        with library.open('xb') as output:
            output.write(binary)
    return imports


def decoder_receipt(destination, imports):
    def dependency(name):
        path = destination / name
        if not path.is_file() or path.is_symlink():
            raise ValueError(f'missing or linked libFLAC dependency: {name}')
        return dict(name=name, bytes=path.stat().st_size, sha256=sha256(path))

    receipt = {
        'version': FLAC_VERSION,
        'sourceArchiveSha256': FLAC_ARCHIVE_SHA256,
        'library': dependency(FLAC),
        'license': {'path': FLAC_LICENSE, 'sha256': sha256(destination / FLAC_LICENSE)},
        'package': {'name': FLAC_PACKAGE, 'release': FLAC_RELEASE, 'url': FLAC_URL},
        'pe': {'machine': 'x86_64', 'imports': sorted(imports),
               'dependencies': [dependency(name) for name in sorted(FLAC_IMPORTS)]},
    }
    decoder_members(receipt)
    return receipt


def decoder_members(receipt):
    """Validate the locked identity and return exact packaged members to hash."""
    if (
        receipt['version'] != FLAC_VERSION
        or receipt['sourceArchiveSha256'] != FLAC_ARCHIVE_SHA256
        or receipt['library'] != dict(name=FLAC, bytes=FLAC_DLL_BYTES, sha256=FLAC_DLL_SHA256)
        or receipt['license'] != dict(path=FLAC_LICENSE, sha256=FLAC_LICENSE_SHA256)
        or receipt['package'] != dict(name=FLAC_PACKAGE, release=FLAC_RELEASE, url=FLAC_URL)
        or receipt['pe']['machine'] != 'x86_64'
        or receipt['pe']['imports'] != sorted(PE_IMPORTS)
    ):
        raise ValueError('wrong pinned Windows FLAC decoder receipt')
    dependencies = receipt['pe']['dependencies']
    if ([row['name'] for row in dependencies] != sorted(FLAC_IMPORTS)
            or any(type(row['bytes']) is not int or not 0 < row['bytes'] <= 2 * 1024**2
                   or not re.fullmatch(r'[0-9a-f]{64}', row['sha256']) for row in dependencies)):
        raise ValueError('wrong Windows FLAC dependency receipt')
    return {
        FLAC: (FLAC_DLL_BYTES, FLAC_DLL_SHA256),
        FLAC_LICENSE: (FLAC_LICENSE_BYTES, FLAC_LICENSE_SHA256),
        **{row['name']: (row['bytes'], row['sha256']) for row in dependencies},
    }
