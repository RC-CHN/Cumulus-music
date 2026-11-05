"""Launch the exact Windows portable ZIP runtime in graphics diagnostic mode."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile

from windows_flac import FLAC, FLAC_IMPORTS, decoder_members


PLATFORM = 'windows-x64'
REQUIRED = {'cumulus.exe', 'ffprobe.exe', 'cumulus_audio_filter.dll', 'libmpv-2.dll'}
DEVICE_NAMES = {'con', 'prn', 'aux', 'nul', *(f'com{i}' for i in range(1, 10)),
                *(f'lpt{i}' for i in range(1, 10))}


def verify_packaged_decoder(package, prefix, members, manifest, runtime, required):
    receipt = runtime.get('flacDecoder')
    if not required and receipt is None and manifest.get('flacDecoder') is None:
        return
    assert receipt and receipt == manifest.get('flacDecoder'), 'missing or mismatched Windows FLAC manifest'
    assert {FLAC.lower(), *FLAC_IMPORTS} <= set(runtime['dlls']), 'runtime manifest omits the FLAC closure'
    expected = decoder_members(receipt)
    expected_lower = {name.lower() for name in expected}
    present = {}
    for member in members:
        leaf = member.filename[len(prefix):]
        key = leaf.lower()
        if key not in expected_lower:
            continue
        assert not member.is_dir() and leaf in expected and leaf not in present, (
            'wrong or duplicate Windows FLAC ZIP member')
        present[leaf] = member
    assert present.keys() == expected.keys(), 'portable ZIP missing pinned FLAC or its notice'
    for name, (size, digest) in expected.items():
        member = present[name]
        assert member.file_size == size, f'wrong Windows FLAC ZIP member size: {name}'
        with package.open(member) as source:
            assert hashlib.file_digest(source, 'sha256').hexdigest() == digest, (
                f'wrong Windows FLAC ZIP member checksum: {name}')


def portable_runtime(packages, commit, temporary, *, ci_run=None, ci_attempt=None, require_package_flac=False):
    manifest = json.loads((packages / f'manifest-{PLATFORM}.json').read_text(encoding='utf-8'))
    version = manifest['version']
    assert re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version), 'invalid package version'
    assert manifest['commit'] == commit and manifest['platform'] == PLATFORM, 'wrong package provenance'
    for key, explicit, env in [('ciRun', ci_run, 'GITHUB_RUN_ID'),
                               ('ciAttempt', ci_attempt, 'GITHUB_RUN_ATTEMPT')]:
        expected = explicit if explicit is not None else os.environ.get(env)
        if expected is not None:
            assert manifest[key] == expected, f'wrong package {key}'
    top = f'Cumulus-{version}-{PLATFORM}'
    name = f'{top}.zip'
    assets = [asset for asset in manifest['assets'] if asset['name'] == name]
    assert len(assets) == 1, 'missing or duplicate portable ZIP manifest entry'
    archive = packages / name
    assert archive.stat().st_size == assets[0]['bytes'], 'portable ZIP size mismatch'
    with archive.open('rb') as stream:
        assert hashlib.file_digest(stream, 'sha256').hexdigest() == assets[0]['sha256'], 'portable ZIP checksum mismatch'

    portable = temporary / top
    with zipfile.ZipFile(archive) as package:
        members = package.infolist()
        prefix = f'{top}/'
        assert members and all(info.filename.startswith(prefix) for info in members), 'wrong portable ZIP root'
        # Only copy the known runtime entries. The shipped provenance and
        # nested licenses are not needed to launch and are not extracted.
        flat = {}
        for info in members:
            leaf = info.filename[len(prefix):]
            if not info.is_dir() and '/' not in leaf:
                key = leaf.lower()
                assert key not in flat, 'duplicate top-level portable ZIP file'
                flat[key] = info
        assert 'runtime-manifest.json' in flat, 'portable ZIP missing runtime manifest'
        metadata = flat['runtime-manifest.json']
        assert metadata.file_size <= 1024 * 1024, 'runtime manifest too large'
        runtime = json.loads(package.read(metadata))
        assert (runtime['platform'], runtime['commit'], runtime['version']) == (PLATFORM, commit, version), 'wrong runtime provenance'
        dlls = runtime['dlls']
        assert isinstance(dlls, list) and len(dlls) == len(set(dlls)), 'invalid runtime DLL list'
        assert 'libmpv-2.dll' in dlls, 'runtime manifest omits libmpv'
        for dll in dlls:
            assert re.fullmatch(r'[a-z0-9][a-z0-9._+-]*\.dll', dll) and dll.split('.')[0] not in DEVICE_NAMES, 'invalid runtime DLL name'
        verify_packaged_decoder(package, prefix, members, manifest, runtime, require_package_flac)
        expected = REQUIRED | set(dlls)
        assert expected <= flat.keys(), 'portable ZIP missing adjacent runtime files'
        assert {leaf for leaf in flat if leaf.endswith('.dll')} == {leaf for leaf in expected if leaf.endswith('.dll')}, 'portable ZIP DLLs differ from runtime manifest'
        portable.mkdir()
        for leaf in sorted(expected):
            info = flat[leaf]
            filename = info.filename[len(prefix):]
            assert filename.isascii(), 'invalid portable runtime filename'
            with package.open(info) as source, (portable / filename).open('xb') as output:
                shutil.copyfileobj(source, output)
    return portable


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--packages', required=True, type=Path)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--root', required=True, type=Path)
    args = parser.parse_args()
    assert os.name == 'nt', 'launch must run on Windows'
    assert re.fullmatch(r'[0-9a-f]{40}', args.commit), 'expected full commit SHA'
    root = args.root.resolve()
    assert not root.exists(), 'new proof root required'
    graphics = Path(__file__).with_name('graphics_smoke.py').resolve()
    with tempfile.TemporaryDirectory(prefix='cumulus-windows-zip-') as folder:
        portable = portable_runtime(
            args.packages.resolve(), args.commit, Path(folder), require_package_flac=True,
        )
        system = Path(os.environ['SystemRoot'])
        env = {key: value for key, value in os.environ.items()
               if not key.upper().startswith('CUMULUS_')
               and key.upper() not in {'PATH', 'MPV_DLL_DIR_CUMULUS'}}
        env['PATH'] = os.pathsep.join(map(str, (portable, system / 'System32', system)))
        subprocess.run(
            [sys.executable, str(graphics), '--binary', str(portable / 'cumulus.exe'),
             '--root', str(root)], cwd=portable, env=env, check=True, timeout=240,
        )
    print(json.dumps(dict(ok=True, exactWindowsZip=True, commit=args.commit)))


if __name__ == '__main__':
    main()
