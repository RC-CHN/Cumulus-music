"""Transfer only the exact CI-built Linux WebView probe and its receipt."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile


BINARY = 'ux_ui_probe'
RECEIPT = 'receipt.json'
MAX_BINARY = 384 * 1024 * 1024
MAX_DIST = 64 * 1024 * 1024
MAX_FILES = 2048
MAX_RECEIPT = 2048


def run_identity(env, checkout):
    commit, run, attempt = (env.get(name, '') for name in
                            ('GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'))
    if (commit != checkout or not re.fullmatch(r'[0-9a-f]{40}', commit)
            or not all(re.fullmatch(r'[1-9][0-9]{0,19}', value) for value in (run, attempt))):
        raise ValueError('invalid Linux WebView probe checkout/run/attempt')
    return {'commit': commit, 'ciRun': run, 'ciAttempt': attempt}


def checked_size(path, maximum):
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or not 0 < info.st_size <= maximum:
        raise ValueError(f'invalid Linux WebView artifact file: {path.name}')
    return info.st_size


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def frontend_digest(root):
    if not stat.S_ISDIR(root.lstat().st_mode):
        raise ValueError('invalid Linux WebView frontend directory')
    files = []
    available = MAX_DIST
    for count, path in enumerate(root.rglob('*'), 1):
        if count > MAX_FILES:
            raise ValueError('too many Linux WebView frontend assets')
        if stat.S_ISDIR(path.lstat().st_mode):
            continue
        available -= checked_size(path, available)
        files.append((path.relative_to(root).as_posix(), digest(path)))
    if not any(name == 'index.html' for name, _ in files):
        raise ValueError('missing Linux WebView frontend entry')
    return hashlib.sha256(json.dumps(sorted(files), ensure_ascii=True).encode()).hexdigest()


def binary_receipt(binary, name):
    size = checked_size(binary, MAX_BINARY)
    with binary.open('rb') as file:
        header = file.read(64)
    if (len(header) != 64 or header[:7] != b'\x7fELF\x02\x01\x01'
            or header[16:18] not in (b'\x02\x00', b'\x03\x00') or header[18:20] != b'\x3e\x00'):
        raise ValueError('Linux WebView probe must be a Linux x86_64 ELF executable')
    return {'name': name, 'bytes': size, 'sha256': digest(binary)}


def receipt(binary, dist, identity):
    result = {
        'kind': 'linux-webview-ux-probe-v1',
        **identity,
        'frontendSha256': frontend_digest(dist),
        'binary': binary_receipt(binary, BINARY),
    }
    return (json.dumps(result, sort_keys=True) + '\n').encode()


def stage(binary, dist, artifact, identity):
    checked_size(binary, MAX_BINARY)
    if not binary.stat().st_mode & stat.S_IXUSR:
        raise ValueError('Linux WebView probe was not built executable')
    artifact.mkdir()
    staged = artifact / BINARY
    shutil.copyfile(binary, staged)
    verified = receipt(staged, dist, identity)
    (artifact / RECEIPT).write_bytes(verified)
    return verified


def restore(artifact, dist, binary, identity):
    if not stat.S_ISDIR(artifact.lstat().st_mode) or {p.name for p in artifact.iterdir()} != {BINARY, RECEIPT}:
        raise ValueError('unexpected Linux WebView probe artifact contents')
    manifest = artifact / RECEIPT
    checked_size(manifest, MAX_RECEIPT)
    source = artifact / BINARY
    checked_size(source, MAX_BINARY)
    binary.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=binary.parent) as temporary:
        staged = Path(temporary) / BINARY
        shutil.copyfile(source, staged)
        verified = receipt(staged, dist, identity)
        if manifest.read_bytes() != verified:
            raise ValueError('Linux WebView probe receipt mismatch (checkout/run/attempt/assets/binary)')
        # Artifact download can lose executable mode. Install only the checked
        # bytes, with no window in which an unverified binary can be launched.
        staged.chmod(0o755)
        os.link(staged, binary)
    return verified


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('stage', 'restore'))
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path, required=True)
    parser.add_argument('--artifact', type=Path, required=True)
    args = parser.parse_args()
    checkout = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    identity = run_identity(os.environ, checkout)
    verified = (
        stage(args.binary, args.frontend_dist, args.artifact, identity)
        if args.action == 'stage'
        else restore(args.artifact, args.frontend_dist, args.binary, identity)
    )
    print(verified.decode(), end='')


if __name__ == '__main__':
    main()
