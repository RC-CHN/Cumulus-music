"""Stage and attest the exact Linux FLAC files that go into the Debian and portable bundles."""
import hashlib
import json
from pathlib import PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tempfile

import linux_flac150_package as candidate
from linux_flac150 import ARCHIVE_SHA256, URL, VERSION, sha256

LIBRARY = candidate.SONAME
NOTICE = 'third-party/licenses/flac-1.5.0/COPYING.Xiph'
DEBIAN_NATIVE = 'usr/lib/Cumulus/native'
COMPILE_DIGEST_ENV = 'CUMULUS_PACKAGED_LINUX_FLAC_SHA256'
MAX_NOTICE = 16 * 1024
MAX_MANIFEST = 1024 * 1024
SHA256 = re.compile(r'[0-9a-f]{64}')


def validate_receipt(receipt):
    if not isinstance(receipt, dict):
        raise ValueError('Linux package is missing its FLAC decoder receipt')
    if (receipt.get('version'), receipt.get('sourceArchiveSha256'), receipt.get('sourceUrl')) != (
        VERSION, ARCHIVE_SHA256, URL
    ):
        raise ValueError('Linux package FLAC source pin mismatch')
    library = receipt.get('library')
    if (not isinstance(library, dict) or library.get('name') != LIBRARY
            or type(library.get('bytes')) is not int
            or not 0 < library['bytes'] <= candidate.MAX_LIBRARY
            or not isinstance(library.get('sha256'), str)
            or not SHA256.fullmatch(library['sha256'])):
        raise ValueError('Linux package FLAC runtime receipt is invalid')
    if receipt.get('license') != {'path': NOTICE, 'sha256': candidate.NOTICE_SHA256}:
        raise ValueError('Linux package FLAC license pin mismatch')
    elf = receipt.get('elf')
    if (not isinstance(elf, dict) or elf.get('soname') != LIBRARY
            or elf.get('needed') != sorted(candidate.SYSTEM_IMPORTS)
            or elf.get('symbolicBinding') is not True or elf.get('flacSymbolRelocations') != 0
            or elf.get('searchTags') != []):
        raise ValueError('Linux package FLAC ELF attestation missing')
    native = receipt.get('native')
    if (not isinstance(native, dict) or native.get('version') != VERSION
            or native.get('libraryIdentityVerified') is not True
            or native.get('decoderAllocatedAndDeleted') is not True
            or native.get('loaderTraceVerified') is not True
            or native.get('foreignFlacSymbolBindings') != 0):
        raise ValueError('Linux package FLAC loader attestation missing')
    return library


def stage_decoder(native, work, *, locked_archive=None):
    """Build from the pinned source, copy only the two approved release files."""
    library = candidate.build(work, locked_archive=locked_archive)
    report = json.loads((work / 'report.json').read_text(encoding='utf-8'))
    if (report.get('ok') is not True or report.get('packageCandidate') is not True
            or report.get('packaged') is not False or library != work / 'runtime' / LIBRARY):
        raise ValueError('unexpected Linux FLAC candidate build receipt')
    receipt = {
        'version': report['version'],
        'sourceUrl': report['sourceUrl'],
        'sourceArchiveSha256': report['sourceArchiveSha256'],
        'library': {'name': LIBRARY, 'bytes': report['libraryBytes'],
                    'sha256': report['librarySha256']},
        'license': {'path': NOTICE, 'sha256': report['license']['sha256']},
        'elf': report['elf'],
        'native': report['native'],
        'build': {key: report[key] for key in ('cmakeVersion', 'cmakeOptions', 'cmakePatchSha256')},
    }
    validate_receipt(receipt)
    if report['license']['path'] != f'runtime/{NOTICE}':
        raise ValueError('unexpected Linux FLAC license location')
    notice = native / NOTICE
    notice.parent.mkdir(parents=True, exist_ok=False)
    for source, destination in ((library, native / LIBRARY), (library.parent / NOTICE, notice)):
        with source.open('rb') as contents, destination.open('xb') as output:
            shutil.copyfileobj(contents, output)
    verify_extracted(native, receipt)
    return receipt


def verify_extracted(directory, receipt, *, staged_library=None, native_probe=False):
    """Verify the extracted package's bytes; optionally exercise that exact library."""
    library = validate_receipt(receipt)
    target, notice = directory / LIBRARY, directory / NOTICE
    candidate.bounded_file(target, candidate.MAX_LIBRARY)
    candidate.bounded_file(notice, MAX_NOTICE)
    if target.stat().st_size != library['bytes'] or sha256(target) != library['sha256']:
        raise ValueError('extracted Linux FLAC differs from the verified runtime')
    if sha256(notice) != candidate.NOTICE_SHA256:
        raise ValueError('extracted Linux FLAC notice differs from the pinned source')
    if staged_library is not None:
        candidate.bounded_file(staged_library, candidate.MAX_LIBRARY)
        if sha256(staged_library) != library['sha256']:
            raise ValueError('extracted Linux FLAC differs from this package stage')
    checked = {'sha256': library['sha256'], 'bytes': library['bytes'],
               'licenseSha256': candidate.NOTICE_SHA256,
               'matchesPackageStage': staged_library is not None}
    if native_probe:
        elf = candidate.audit_elf(target)
        if elf != receipt['elf']:
            raise ValueError('extracted Linux FLAC ELF differs from the candidate attestation')
        checked['native'] = candidate.native_probe(target)
        if sha256(target) != library['sha256']:
            raise ValueError('extracted Linux FLAC bytes changed during loader audit')
    return checked


def verify_members(package, directory, receipt, *, portable_metadata=None):
    library = validate_receipt(receipt)
    library_name = str(PurePosixPath(directory) / LIBRARY)
    notice_name = str(PurePosixPath(directory) / NOTICE)
    expected = {library_name, notice_name}
    metadata_name = str(PurePosixPath(directory) / 'runtime-manifest.json')
    if portable_metadata is not None:
        expected.add(metadata_name)
    found = set()
    for member in package:
        name = member.name.removeprefix('./')
        if PurePosixPath(name).name == LIBRARY and name != library_name:
            raise ValueError('Linux package has an extra FLAC runtime location')
        if name not in expected:
            continue
        if name in found or not member.isfile():
            raise ValueError('Linux package has a duplicate or linked FLAC member')
        found.add(name)
        if name == library_name and member.size != library['bytes']:
            raise ValueError('Linux package FLAC library size mismatch')
        limit = MAX_MANIFEST if name == metadata_name else MAX_NOTICE if name == notice_name else candidate.MAX_LIBRARY
        if not 0 < member.size <= limit:
            raise ValueError('Linux package FLAC member exceeds bounds')
        with package.extractfile(member) as stream:
            if name == metadata_name:
                value = json.loads(stream.read())
                if (not isinstance(value, dict)
                        or any(value.get(key) != expected_value for key, expected_value in portable_metadata.items())
                        or value.get('flacDecoder') != receipt):
                    raise ValueError('portable and outer Linux FLAC receipts differ')
            else:
                expected_sha = library['sha256'] if name == library_name else candidate.NOTICE_SHA256
                if hashlib.file_digest(stream, 'sha256').hexdigest() != expected_sha:
                    raise ValueError('Linux package FLAC member SHA256 mismatch')
    if found != expected:
        raise ValueError('Linux package is missing the verified FLAC runtime, notice or receipt')


def verify_archives(debian, portable, prefix, receipt, *, commit, version):
    """Verify the actual bundle members before recording the whole-asset hashes."""
    with tarfile.open(portable, 'r|gz') as package:
        verify_members(package, prefix, receipt, portable_metadata={
            'commit': commit, 'version': version, 'platform': 'linux-x64',
        })
    # Use dpkg's real compression support without unpacking executable contents
    # to the filesystem. The temporary tar is removed after bounded execution.
    with tempfile.TemporaryFile() as payload:
        subprocess.run(['dpkg-deb', '--fsys-tarfile', debian], stdout=payload,
                       check=True, timeout=60)
        payload.seek(0)
        with tarfile.open(fileobj=payload, mode='r|') as package:
            verify_members(package, DEBIAN_NATIVE, receipt)
