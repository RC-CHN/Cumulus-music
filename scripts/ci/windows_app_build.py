"""Bind the early Windows release executable to its CI attempt and inputs."""

import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess

from windows_flac import FLAC_DLL_SHA256
from windows_package_probes import executable_receipt
from windows_webview_inputs import FRONTEND, ci_identity, file_receipt, frontend_receipts, plain_stat


ARTIFACT = Path('.build/windows-release-app')
BUNDLE_CONFIG = Path('.build/package-stage/windows-x64/bundle-config.json')
EXE = 'cumulus.exe'
MANIFEST = 'manifest.json'
NULL_PROOF_ENV = 'CUMULUS_BUILD_WINDOWS_NATIVE_OPEN_NULL_PROOF'
MAX_MANIFEST = 2 * 1024 * 1024
MAX_CONFIG = 64 * 1024


def null_proof_enabled(env):
    # desktop/build.rs enables the diagnostic for exactly this string.
    return env.get(NULL_PROOF_ENV) == '1'


def config_receipt(config):
    info = plain_stat(config)
    assert stat.S_ISREG(info.st_mode) and 0 < info.st_size <= MAX_CONFIG, 'invalid Windows bundle config'
    result = file_receipt(config)
    assert result['bytes'] == info.st_size, 'Windows bundle config changed while hashing'
    return result


def receipt(executable, identity, frontend_dist, *, null_proof: bool, config):
    assert type(null_proof) is bool, 'expected a Boolean native-open proof mode'
    return dict(kind='windows-x64-release-app-v1', **identity,
                nativeOpenNullProof=null_proof, flacSha256=FLAC_DLL_SHA256,
                bundleConfig=config_receipt(config),
                executable=executable_receipt(executable), frontend=frontend_receipts(frontend_dist))


def stage(repo, identity, *, target_dir, null_proof: bool):
    executable = repo / target_dir / 'release' / EXE
    expected = receipt(executable, identity, repo / FRONTEND, null_proof=null_proof, config=repo / BUNDLE_CONFIG)
    manifest = (json.dumps(expected, sort_keys=True, separators=(',', ':')) + '\n').encode('utf-8')
    assert len(manifest) <= MAX_MANIFEST, 'Windows release app manifest is too large'
    artifact = repo / ARTIFACT
    artifact.mkdir(parents=True)
    shutil.copyfile(executable, artifact / EXE)
    assert executable_receipt(artifact / EXE) == expected['executable'], 'release app changed while staging'
    with (artifact / MANIFEST).open('xb') as output:
        output.write(manifest)


def verify(artifact, identity, frontend_dist, *, null_proof: bool, config) -> Path:
    assert stat.S_ISDIR(plain_stat(artifact).st_mode), 'expected the Windows release app artifact directory'
    assert {item.name for item in artifact.iterdir()} == {EXE, MANIFEST}, 'unexpected Windows release app artifact contents'
    path = artifact / MANIFEST
    info = plain_stat(path)
    assert stat.S_ISREG(info.st_mode) and 0 < info.st_size <= MAX_MANIFEST, 'invalid Windows release app manifest'
    stored = json.loads(path.read_text(encoding='utf-8'))
    executable = artifact / EXE
    assert isinstance(stored, dict) and type(stored.get('nativeOpenNullProof')) is bool, 'invalid native-open proof mode'
    expected = receipt(executable, identity, frontend_dist, null_proof=null_proof, config=config)
    assert stored == expected, 'Windows release app provenance, build mode or checksum mismatch'
    return executable


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('stage',))
    args = parser.parse_args()
    assert os.name == 'nt', 'stage the Windows release app on Windows'
    repo = Path.cwd()
    identity = ci_identity(os.environ)
    assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip() == identity['commit'], (
        'checkout does not match the Windows release app CI attempt')
    stage(repo, identity, target_dir=Path(os.environ.get('CARGO_TARGET_DIR', '.build/target')),
          null_proof=null_proof_enabled(os.environ))
    print(json.dumps(dict(operation=args.operation, **identity)))


if __name__ == '__main__':
    main()
