"""Stage a candidate Linux package's pinned FLAC 1.5.0, without building a package."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import stat
import subprocess
import sys

from linux_flac150 import ARCHIVE_SHA256, MAX_ARCHIVE, OPTIONS, URL, VERSION, download, sha256, unpack

SONAME = 'libcumulus_flac150.so.14'
NOTICE_SHA256 = '7866ee98760fc1f0156b4fe6bf530257e02be487ab3fd94e2b63799dd32d6b2c'
MAX_LIBRARY = 8 * 1024 * 1024
PACKAGE_OPTIONS = (*OPTIONS, '-DCMAKE_C_FLAGS=-march=x86-64 -mtune=generic',
                   '-DCMAKE_SHARED_LINKER_FLAGS=')
CMAKE_ISOLATION = b"""
# Cumulus package candidate: isolate data and functions from ambient libFLAC.
set_target_properties(FLAC PROPERTIES OUTPUT_NAME cumulus_flac150)
target_link_options(FLAC PRIVATE "LINKER:-Bsymbolic")
"""
SYSTEM_IMPORTS = {'libc.so.6', 'libm.so.6'}
FORBIDDEN_TAGS = {'RPATH', 'RUNPATH', 'AUDIT', 'DEPAUDIT', 'FILTER', 'AUXILIARY', 'TEXTREL'}
DECODER_SYMBOLS = tuple(f'FLAC__stream_decoder_{name}' for name in (
    'new', 'delete', 'init_stream', 'process_until_end_of_metadata', 'process_single',
    'seek_absolute', 'get_total_samples', 'get_state', 'finish',
))
VERSION_SYMBOL = 'FLAC__VERSION_STRING'
BINDING = re.compile(
    r'^\s*\d+:\s+binding file (.+?) \[\d+\] to (.+?) \[\d+\]:.* symbol `(FLAC__[^\']+)\'$',
)
READELF_TAG = re.compile(r'^\s*0x[0-9a-f]+\s+\(([A-Z0-9_]+)\)\s+(.*)$')


def bounded_file(path, cap):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= cap:
        raise ValueError(f'expected bounded regular file: {path.name}')


def prepare_archive(destination, locked_archive):
    if locked_archive is None:
        download(destination)
    else:
        bounded_file(locked_archive, MAX_ARCHIVE)
        with locked_archive.open('rb') as source:
            payload = source.read(MAX_ARCHIVE + 1)
        if (len(payload) > MAX_ARCHIVE
                or hashlib.sha256(payload).hexdigest() != ARCHIVE_SHA256):
            raise ValueError('locked FLAC archive SHA256 mismatch')
        with destination.open('xb') as target:
            target.write(payload)


def dynamic_contract(raw):
    entries = [match.groups() for line in raw.splitlines() if (match := READELF_TAG.match(line))]
    needed, sonames = [], []
    for tag, value in entries:
        if tag in FORBIDDEN_TAGS or (tag in {'FLAGS', 'FLAGS_1'} and 'TEXTREL' in value):
            raise ValueError(f'packaged FLAC contains forbidden ELF tag {tag}')
        if tag in {'NEEDED', 'SONAME'}:
            bracket = re.fullmatch(r'[^\[]+\[([^\]]+)\]\s*', value)
            if bracket is None:
                raise ValueError('could not read ELF dependency name')
            (needed if tag == 'NEEDED' else sonames).append(bracket[1])
    if sonames != [SONAME] or sorted(needed) != sorted(SYSTEM_IMPORTS):
        raise ValueError('wrong FLAC SONAME or unexpected ELF dependency closure')
    if not any(tag == 'SYMBOLIC' or (tag == 'FLAGS' and 'SYMBOLIC' in value)
               for tag, value in entries):
        raise ValueError('packaged FLAC lacks symbolic binding')
    return sorted(needed)


def export_contract(symbols, relocations):
    if not re.search(r'\bR_X86_64_(?:RELATIVE|GLOB_DAT|JUMP_SLOT)\b', relocations):
        raise ValueError('could not audit x86_64 ELF dynamic relocations')
    if re.search(r'\bFLAC__', relocations) or re.search(r'\bUND\s+FLAC__', symbols):
        raise ValueError('packaged FLAC has an interposable FLAC relocation or import')
    exports = {
        match[2]: match[1] for line in symbols.splitlines()
        if (match := re.match(r'^\s*\d+:\s+\w+\s+\d+\s+(FUNC|OBJECT)\s+GLOBAL\s+DEFAULT\s+\d+\s+(\w+)\s*$', line))
    }
    if exports.get(VERSION_SYMBOL) != 'OBJECT' or any(exports.get(name) != 'FUNC' for name in DECODER_SYMBOLS):
        raise ValueError('packaged FLAC lacks the expected public decoder ABI exports')


def audit_elf(path):
    """Check bytes structurally before any native initializer can execute."""
    bounded_file(path, MAX_LIBRARY)
    with path.open('rb') as library:
        header = library.read(64)
    if (len(header) != 64 or header[:7] != b'\x7fELF\x02\x01\x01'
            or header[16:20] != b'\x03\x00\x3e\x00'):
        raise ValueError('packaged FLAC requires a Linux x86_64 shared ELF')

    def readelf(option):
        return subprocess.check_output(
            ['readelf', '--wide', option, path], env={**os.environ, 'LC_ALL': 'C'},
            text=True, encoding='utf-8', timeout=20,
        )

    needed = dynamic_contract(readelf('--dynamic'))
    export_contract(readelf('--dyn-syms'), readelf('--relocs'))
    names = set(re.findall(r'\bName: (GLIBC_[^\s]+)', readelf('--version-info')))
    if not names or any(not re.fullmatch(r'GLIBC_\d+(?:\.\d+)+', name) for name in names):
        raise ValueError('unexpected FLAC glibc version requirement')
    ordered = sorted(names, key=lambda name: tuple(map(int, name[6:].split('.'))))
    if tuple(map(int, ordered[-1][6:].split('.'))) > (2, 39):
        raise ValueError('FLAC requires a newer glibc than Ubuntu 24.04')
    return {'soname': SONAME, 'needed': needed, 'symbolicBinding': True,
            'flacSymbolRelocations': 0, 'searchTags': [], 'glibcMaxRequired': ordered[-1],
            'decoderSymbols': [VERSION_SYMBOL, *DECODER_SYMBOLS]}


PROBE = r"""
import ctypes
import json
import os
import sys

class DlInfo(ctypes.Structure):
    _fields_ = [('name', ctypes.c_char_p), ('base', ctypes.c_void_p),
                ('symbol', ctypes.c_char_p), ('address', ctypes.c_void_p)]

def verify_owner(symbol, expected):
    info = DlInfo()
    if not posix.dladdr(symbol, ctypes.byref(info)) or not os.path.samefile(info.name, expected):
        raise RuntimeError('native FLAC symbol came from a different file')

path, ambient, *exports = sys.argv[1:]
posix = ctypes.CDLL(None)
posix.dladdr.argtypes = [ctypes.c_void_p, ctypes.POINTER(DlInfo)]
posix.dladdr.restype = ctypes.c_int
if ambient:
    other = ctypes.CDLL(ambient, mode=os.RTLD_NOW | os.RTLD_GLOBAL)
    verify_owner(ctypes.cast(other.FLAC__stream_decoder_new, ctypes.c_void_p), ambient)
flac = ctypes.CDLL(path, mode=os.RTLD_NOW | os.RTLD_LOCAL)
version = ctypes.c_char_p.in_dll(flac, 'FLAC__VERSION_STRING')
verify_owner(ctypes.addressof(version), path)
if version.value != b'1.5.0':
    raise RuntimeError('native FLAC version must be exactly 1.5.0')
for export in exports:
    verify_owner(ctypes.cast(getattr(flac, export), ctypes.c_void_p), path)
create = flac.FLAC__stream_decoder_new
create.argtypes = []
create.restype = ctypes.c_void_p
delete = flac.FLAC__stream_decoder_delete
delete.argtypes = [ctypes.c_void_p]
delete.restype = None
decoder = create()
if not decoder:
    raise RuntimeError('native FLAC decoder allocation failed')
delete(decoder)
print(json.dumps({'version': version.value.decode(), 'libraryIdentityVerified': True,
                  'decoderAllocatedAndDeleted': True, 'ambientPreloaded': bool(ambient)}))
"""


def binding_contract(debug, library, ambient):
    observed = [match.groups() for line in debug.splitlines() if (match := BINDING.match(line))]
    if not any(target == library and name == DECODER_SYMBOLS[0] for _, target, name in observed):
        raise ValueError('loader trace has no package decoder symbol receipt')
    if ambient and not any(target == ambient and name == DECODER_SYMBOLS[0] for _, target, name in observed):
        raise ValueError('loader trace has no ambient decoder symbol receipt')
    foreign = [(target, name) for source, target, name in observed if source == library and target != library]
    if foreign:
        raise ValueError(f'packaged FLAC internally bound {len(foreign)} FLAC symbols to another file')
    return {'foreignFlacSymbolBindings': 0, 'loaderTraceVerified': True}


def native_probe(library, ambient_library=None):
    """Build-host-only identity/relocation proof; not a desktop runtime selector."""
    library = str(library.resolve(strict=True))
    if ambient_library is not None:
        ambient = str(ambient_library.resolve(strict=True))
        if os.path.samefile(library, ambient):
            raise ValueError('ambient FLAC must be a different trusted system file')
    else:
        ambient = ''
    env = {key: value for key, value in os.environ.items() if not key.startswith('LD_')}
    child = subprocess.run(
        [sys.executable, '-I', '-c', PROBE, library, ambient, *DECODER_SYMBOLS],
        env={**env, 'LD_DEBUG': 'bindings'}, text=True, encoding='utf-8',
        capture_output=True, check=True, timeout=30,
    )
    return {**json.loads(child.stdout), **binding_contract(child.stderr, library, ambient)}


def build(root, locked_archive=None, ambient_library=None):
    """Create a new package candidate stage and its receipt, without modifying a bundle."""
    if platform.system() != 'Linux' or platform.machine() not in {'x86_64', 'AMD64'}:
        raise ValueError('the FLAC package candidate requires Linux x86_64')
    root.mkdir(parents=True, exist_ok=False)
    report = {'ok': False, 'packageCandidate': True, 'packaged': False,
              'sourceUrl': URL, 'sourceArchiveSha256': ARCHIVE_SHA256, 'version': VERSION,
              'cmakeOptions': list(PACKAGE_OPTIONS), 'globalInstallPerformed': False}
    try:
        archive = root / 'flac-1.5.0.tar.xz'
        prepare_archive(archive, locked_archive)
        source = unpack(archive, root / 'source')
        notice = source / 'COPYING.Xiph'
        bounded_file(notice, 16 * 1024)
        if sha256(notice) != NOTICE_SHA256:
            raise ValueError('Xiph 1.5.0 COPYING.Xiph does not match the reviewed notice')
        target = source / 'src/libFLAC/CMakeLists.txt'
        with target.open('ab') as source_file:
            source_file.write(CMAKE_ISOLATION)
        build_dir = root / 'cmake-build'
        with (root / 'build.log').open('x', encoding='utf-8') as log:
            for command in [
                ['cmake', '-S', source, '-B', build_dir, *PACKAGE_OPTIONS],
                ['cmake', '--build', build_dir, '--target', 'FLAC', '--parallel', '2'],
            ]:
                subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True, timeout=300)
        built = build_dir / 'src/libFLAC' / (SONAME + '.0.0')
        bounded_file(built, MAX_LIBRARY)
        runtime = root / 'runtime'
        license_dir = runtime / 'third-party/licenses/flac-1.5.0'
        license_dir.mkdir(parents=True)
        library = runtime / SONAME
        shutil.copyfile(built, library)
        shutil.copyfile(notice, license_dir / notice.name)
        digest = sha256(library)
        elf = audit_elf(library)
        native = native_probe(library, ambient_library)
        if sha256(library) != digest or sha256(license_dir / notice.name) != NOTICE_SHA256:
            raise ValueError('package candidate bytes changed during verification')
        report.update(library=str(library.relative_to(root)), libraryBytes=library.stat().st_size,
                      librarySha256=digest, elf=elf, native=native,
                      license={'path': str((license_dir / notice.name).relative_to(root)),
                               'sha256': NOTICE_SHA256},
                      cmakePatchSha256=hashlib.sha256(CMAKE_ISOLATION).hexdigest(),
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
    parser.add_argument('--build-dir', required=True, type=Path, help='new package candidate work directory')
    parser.add_argument('--locked-archive', type=Path, help='optional local copy of the exact locked Xiph archive')
    parser.add_argument('--ambient-library', type=Path, help='optional trusted system FLAC to preload in the audit child')
    args = parser.parse_args()
    print(build(args.build_dir.resolve(), args.locked_archive, args.ambient_library))


if __name__ == '__main__':
    main()
