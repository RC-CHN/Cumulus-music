"""Build and bind the four release media probes to one Windows CI attempt."""
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess

from release_guard import git
from windows_webview_inputs import ci_identity, file_receipt, plain_stat


ARTIFACT = Path('.build/windows-release-probes')
MANIFEST = 'manifest.json'
EXAMPLES = ('engine_roundtrip', 'player_probe', 'pinned_audio_probe', 'parametric_probe')
BINARIES = tuple(f'{name}.exe' for name in EXAMPLES)
BUILD = (
    'cargo', 'build', '--release', '--locked',
    '-p', 'cumulus-core', '-p', 'cumulus-audio-filter', '--lib',
    *(arg for name in EXAMPLES for arg in ('--example', name)),
)


def receipt(directory, identity):
    return dict(schema=1, **identity,
                binaries={name: file_receipt(directory / name) for name in BINARIES})


def stage(examples, artifact, identity):
    manifest = receipt(examples, identity)
    artifact.mkdir(parents=True)
    for name in BINARIES:
        shutil.copyfile(examples / name, artifact / name)
    assert manifest == receipt(artifact, identity), 'release probes changed during staging'
    (artifact / MANIFEST).write_text(json.dumps(manifest, sort_keys=True) + '\n', encoding='utf-8')


def verify(artifact, identity):
    assert stat.S_ISDIR(plain_stat(artifact).st_mode), 'expected a release probe artifact directory'
    assert {path.name for path in artifact.iterdir()} == {MANIFEST, *BINARIES}, (
        'unexpected release probe contents'
    )
    manifest = artifact / MANIFEST
    info = plain_stat(manifest)
    assert stat.S_ISREG(info.st_mode) and 0 < info.st_size <= 4096, 'invalid release probe manifest'
    stored = json.loads(manifest.read_text(encoding='utf-8'))
    assert stored == receipt(artifact, identity), 'release probe provenance or checksum mismatch'


def main():
    assert os.name == 'nt', 'build the release probes on Windows'
    repo, identity = Path.cwd(), ci_identity(os.environ)
    assert git('rev-parse', 'HEAD') == identity['commit'], 'checkout differs from release probe commit'
    print('+', ' '.join(BUILD), flush=True)
    subprocess.run(BUILD, check=True)
    assert git('rev-parse', 'HEAD') == identity['commit'], 'checkout changed during release probe build'
    target = Path(os.environ.get('CARGO_TARGET_DIR', '.build/target'))
    stage(repo / target / 'release/examples', repo / ARTIFACT, identity)
    print(json.dumps(dict(ok=True, **identity)))


if __name__ == '__main__':
    main()
