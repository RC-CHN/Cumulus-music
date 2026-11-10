"""Stage and verify the native Windows inputs shared by CI and packaging."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import zipfile

from windows_flac import FLAC, decoder_members
from windows_media import proof_binaries, sha256
from windows_webview_inputs import ci_identity, plain_stat
from windows_zip_smoke import DEVICE_NAMES

ARTIFACT = Path('.build/windows-release-runtime')
RUNTIME = 'native'
MANIFEST = 'manifest.json'
FILTER = 'cumulus_audio_filter.dll'
TOOLS = {FILTER, 'ffprobe.exe'}
MEDIA_PROOF = 'third-party/cumulus-ffmpeg-source/installed.json'


def files_in(directory):
    receipts, seen = {}, set()
    pending = [directory]
    while pending:
        base = pending.pop()
        assert stat.S_ISDIR(plain_stat(base).st_mode), 'expected a runtime directory'
        for path in sorted(base.iterdir()):
            relative = path.relative_to(directory).as_posix()
            key = relative.casefold()
            assert key not in seen and len(seen) < 8192, 'invalid runtime path inventory'
            seen.add(key)
            info = plain_stat(path)
            if stat.S_ISDIR(info.st_mode):
                pending.append(path)
            else:
                assert stat.S_ISREG(info.st_mode), 'non-file Windows runtime member'
                with path.open('rb') as stream:
                    receipts[relative] = dict(bytes=info.st_size, sha256=hashlib.file_digest(stream, 'sha256').hexdigest())
    return receipts


def receipt(directory, identity, dlls, flac):
    files = files_in(directory)
    assert (isinstance(dlls, list) and dlls == sorted(set(dlls)) and
            {FLAC.lower(), 'libmpv-2.dll'} <= set(dlls)), 'invalid shared Windows DLL inventory'
    assert all(re.fullmatch(r'[a-z0-9][a-z0-9._+-]*\.dll', name) and name.split('.')[0] not in DEVICE_NAMES
               for name in dlls), 'invalid shared Windows DLL name'
    assert {name.lower() for name in files if '/' not in name} == {*dlls, *TOOLS}, (
        'shared Windows runtime does not match its DLL inventory')
    for name, (size, sha) in decoder_members(flac).items():
        assert files.get(name) == dict(bytes=size, sha256=sha), 'wrong shared Windows FLAC member'
    check_verified_filter(directory / MEDIA_PROOF, files[FILTER]['sha256'])
    return dict(schema=1, **identity, dlls=dlls, flac=flac, files=files)


def check_verified_filter(receipt_path, filter_hash):
    proof = json.loads(receipt_path.read_text(encoding='utf-8'))
    assert proof['filterLoaded'] is True and proof['nativePcmVerified'] is True, 'Windows DSP media proof failed'
    assert proof['pluginSha256'] == filter_hash, 'shared Windows DSP differs from the runtime-verified filter'


def verify(artifact, identity):
    assert stat.S_ISDIR(plain_stat(artifact).st_mode), 'expected the shared runtime artifact'
    assert {entry.name for entry in artifact.iterdir()} == {MANIFEST, RUNTIME}, 'unexpected runtime artifact contents'
    manifest = artifact / MANIFEST
    info = plain_stat(manifest)
    assert stat.S_ISREG(info.st_mode) and info.st_size <= 2 * 1024**2, 'invalid shared runtime manifest'
    stored = json.loads(manifest.read_text(encoding='utf-8'))
    assert stored == receipt(artifact / RUNTIME, identity, stored['dlls'], stored['flac']), (
        'shared Windows runtime provenance or checksum mismatch')
    return stored


def stage(repo, identity, msys):
    from package import windows_media

    assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip() == identity['commit']
    # The media action already built and verified the exact release filter.
    # The shared archive and its consumers must retain that tested DLL.
    filter_path, _ = proof_binaries('release')
    artifact = repo / ARTIFACT
    runtime = artifact / RUNTIME
    runtime.mkdir(parents=True)
    dlls, flac = windows_media(runtime, msys)
    filter_path = repo / filter_path
    check_verified_filter(runtime / MEDIA_PROOF, sha256(filter_path))
    shutil.copyfile(filter_path, runtime / FILTER)
    manifest = receipt(runtime, identity, dlls, flac)
    (artifact / MANIFEST).write_text(json.dumps(manifest, sort_keys=True) + '\n', encoding='utf-8')


def restore(artifact, identity, destination, *, include_notices):
    manifest = verify(artifact, identity)
    assert not destination.exists() and not destination.is_symlink(), 'expected an unused runtime destination'
    destination.mkdir()
    for relative in manifest['files']:
        if include_notices or '/' not in relative:
            output = destination / relative
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(artifact / RUNTIME / relative, output)
    return manifest


def verify_portable(archive, root, manifest):
    """The portable archive must contain the identical runtime early GUI jobs tested."""
    prefix = f'{root}/'
    with zipfile.ZipFile(archive) as bundle:
        members = bundle.namelist()
        assert len(members) == len(set(name.casefold() for name in members)), 'duplicate portable ZIP member'
        for name, expected in manifest['files'].items():
            member = bundle.getinfo(prefix + name)
            assert member.file_size == expected['bytes'], 'portable runtime member size mismatch'
            with bundle.open(member) as stream:
                assert hashlib.file_digest(stream, 'sha256').hexdigest() == expected['sha256'], (
                    'portable runtime member checksum mismatch')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--msys', type=Path, required=True)
    args = parser.parse_args()
    assert os.name == 'nt', 'stage the Windows media runtime on Windows'
    identity = ci_identity(os.environ)
    stage(Path.cwd(), identity, args.msys.resolve())
    print(json.dumps(dict(ok=True, **identity)))


if __name__ == '__main__':
    main()
