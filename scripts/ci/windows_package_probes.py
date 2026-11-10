"""Build and bind the four release probes run against the exact Windows package."""
import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess
import sys

from windows_release_runtime import ARTIFACT as RUNTIME_ARTIFACT, MEDIA_PROOF, RUNTIME
from windows_webview_inputs import MANIFEST, ci_identity, file_receipt, plain_stat

ARTIFACT = Path('.build/windows-package-probes')
GROUPS = {
    'helpers': ('engine_roundtrip.exe', 'parametric_probe.exe', 'pinned_audio_probe.exe'),
    'verified-player': ('player_probe.exe',),
}


def executable_receipt(path):
    result = file_receipt(path)
    # These native helpers run on the Windows x64 package runner. Reject DLLs,
    # other architectures and archives even if their names end in .exe.
    with path.open('rb') as stream:
        dos = stream.read(64)
        assert len(dos) == 64 and dos[:2] == b'MZ', 'invalid package probe executable'
        pe_offset, = struct.unpack_from('<I', dos, 60)
        assert 64 <= pe_offset <= result['bytes'] - 24, 'invalid package probe PE offset'
        stream.seek(pe_offset)
        header = stream.read(24)
        assert len(header) == 24, 'truncated package probe PE header'
        machine, = struct.unpack_from('<H', header, 4)
        flags, = struct.unpack_from('<H', header, 22)
        assert header[:4] == b'PE\0\0' and machine == 0x8664 and flags & 0x2002 == 0x0002, (
            'expected a Windows x64 package probe executable'
        )
    return result


def verified_player(proof_path, player_hash):
    proof = json.loads(proof_path.read_text(encoding='utf-8'))
    assert proof['filterLoaded'] is True and proof['nativePcmVerified'] is True, (
        'Windows runtime player probe did not pass media verification'
    )
    assert proof.get('playerProbeSha256') == player_hash, 'package player differs from the runtime-verified executable'


def require_checkout(repo, identity):
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip()
    assert head == identity['commit'], 'package probe checkout differs from CI commit'
    assert not subprocess.check_output(
        ['git', 'status', '--porcelain', '--untracked-files=normal'], cwd=repo, text=True
    ), 'package probe checkout changed'


def stage(repo, identity, group, examples):
    require_checkout(repo, identity)
    names = GROUPS[group]
    files = {name: executable_receipt(examples / name) for name in names}
    if group == 'verified-player':
        verified_player(repo / RUNTIME_ARTIFACT / RUNTIME / MEDIA_PROOF, files[names[0]]['sha256'])
    directory = repo / ARTIFACT / group
    directory.mkdir(parents=True)
    for name, expected in files.items():
        output = directory / name
        shutil.copyfile(examples / name, output)
        assert file_receipt(output) == expected, 'package probe changed while staging'
    manifest = dict(schema=1, **identity, group=group, files=files)
    (directory / MANIFEST).write_text(json.dumps(manifest, sort_keys=True) + '\n', encoding='utf-8')


def verify(artifact, identity, media_proof):
    assert stat.S_ISDIR(plain_stat(artifact).st_mode), 'expected a package probe artifact directory'
    assert {path.name for path in artifact.iterdir()} == GROUPS.keys(), 'unexpected package probe groups'
    probes = {}
    for group, names in GROUPS.items():
        directory = artifact / group
        assert stat.S_ISDIR(plain_stat(directory).st_mode), 'expected a package probe group directory'
        assert {path.name for path in directory.iterdir()} == {MANIFEST, *names}, 'unexpected package probe contents'
        manifest = directory / MANIFEST
        info = plain_stat(manifest)
        assert stat.S_ISREG(info.st_mode) and 0 < info.st_size <= 4096, 'invalid package probe manifest'
        files = {name: executable_receipt(directory / name) for name in names}
        assert json.loads(manifest.read_text(encoding='utf-8')) == dict(
            schema=1, **identity, group=group, files=files
        ), 'package probe provenance or checksum mismatch'
        probes.update({name: (directory / name, expected) for name, expected in files.items()})
    verified_player(media_proof, probes['player_probe.exe'][1]['sha256'])
    return probes


def build(repo):
    command = ['cargo', 'build', '--release', '--locked', '-p', 'cumulus-core', '-p', 'cumulus-audio-filter']
    for name in GROUPS['helpers']:
        command.extend(['--example', Path(name).stem])
    subprocess.run(command, cwd=repo, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('group', choices=GROUPS)
    args = parser.parse_args()
    assert sys.platform == 'win32', 'build the Windows package probes on Windows'
    repo, identity = Path.cwd(), ci_identity(os.environ)
    require_checkout(repo, identity)
    if args.group == 'helpers':
        build(repo)
    target = Path(os.environ.get('CARGO_TARGET_DIR', '.build/target'))
    stage(repo, identity, args.group, repo / target / 'release/examples')
    print(json.dumps(dict(ok=True, **identity, group=args.group)))


if __name__ == '__main__':
    main()
