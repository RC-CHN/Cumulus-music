"""Transfer only the Windows release probes that verify the packaged runtime."""

import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import struct
import subprocess

from windows_webview_inputs import ci_identity, file_receipt, plain_stat


ARTIFACT = Path('.build/windows-package-probes')
MANIFEST = 'manifest.json'
PROBES = ('engine_roundtrip', 'player_probe', 'pinned_audio_probe', 'parametric_probe')
EXECUTABLES = tuple(f'{name}.exe' for name in PROBES)
RELEASE_PROBE_BUILD = (
    'cargo', 'build', '--release', '--locked', '-p', 'cumulus-core', '-p', 'cumulus-audio-filter', '--lib',
    *(arg for name in PROBES for arg in ('--example', name)),
)
MAX_BINARY = 384 * 1024 * 1024
MAX_MANIFEST = 4096


def executable_receipt(path):
    info = plain_stat(path)
    assert stat.S_ISREG(info.st_mode) and 90 <= info.st_size <= MAX_BINARY, 'invalid release probe file'
    with path.open('rb') as stream:
        dos = stream.read(64)
        offset = struct.unpack_from('<I', dos, 60)[0]
        assert dos[:2] == b'MZ' and 64 <= offset <= info.st_size - 26, 'invalid release probe PE header'
        stream.seek(offset)
        pe = stream.read(26)
    assert pe[:6] == b'PE\0\0\x64\x86' and pe[24:26] == b'\x0b\x02', 'release probe must be Windows x64 PE32+'
    flags = struct.unpack_from('<H', pe, 22)[0]
    assert flags & 0x0002 and not flags & 0x2000, 'release probe must be an executable, not a DLL'
    return file_receipt(path)


def receipt(directory, identity):
    return dict(kind='windows-x64-package-release-probes-v1', **identity,
                binaries={name: executable_receipt(directory / name) for name in EXECUTABLES})


def stage(examples, artifact, identity):
    expected = receipt(examples, identity)
    artifact.mkdir(parents=True)
    for name in EXECUTABLES:
        shutil.copyfile(examples / name, artifact / name)
    assert receipt(artifact, identity) == expected, 'release probes changed while staging'
    (artifact / MANIFEST).write_text(json.dumps(expected, sort_keys=True) + '\n', encoding='utf-8')


def verify(artifact, identity):
    assert stat.S_ISDIR(plain_stat(artifact).st_mode), 'expected the Windows release probe artifact directory'
    assert {item.name for item in artifact.iterdir()} == {MANIFEST, *EXECUTABLES}, 'unexpected Windows release probe artifact contents'
    path = artifact / MANIFEST
    info = plain_stat(path)
    assert stat.S_ISREG(info.st_mode) and 0 < info.st_size <= MAX_MANIFEST, 'invalid Windows release probe manifest'
    stored = json.loads(path.read_text(encoding='utf-8'))
    assert stored == receipt(artifact, identity), 'Windows release probe provenance or checksum mismatch'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('build', 'stage'))
    args = parser.parse_args()
    assert os.name == 'nt', 'build Windows release probes on Windows'
    identity = ci_identity(os.environ)
    assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == identity['commit']
    if args.operation == 'build':
        # Setup MSYS2 exports MSYSTEM for its launcher. Cargo here uses the
        # same native toolchain as the package job, without that selection.
        env = {key: value for key, value in os.environ.items() if key.upper() != 'MSYSTEM'}
        host = subprocess.check_output(['rustc', '-vV'], text=True, env=env).splitlines()
        assert 'host: x86_64-pc-windows-msvc' in host, 'expected the Windows packaging toolchain'
        print('+', ' '.join(RELEASE_PROBE_BUILD), flush=True)
        subprocess.run(RELEASE_PROBE_BUILD, check=True, env=env)
    else:
        target = Path(os.environ.get('CARGO_TARGET_DIR', '.build/target'))
        stage(target / 'release/examples', ARTIFACT, identity)
    print(json.dumps(dict(operation=args.operation, **identity)))


if __name__ == '__main__':
    main()
