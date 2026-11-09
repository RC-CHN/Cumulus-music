"""Verify the shared probe and portable Windows runtime against the same CI attempt."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import tempfile

from windows_zip_smoke import portable_runtime


ARTIFACT = Path('.build/windows-webview-inputs')
PACKAGES = Path('.build/packages/windows-x64')
NATIVE = Path('.build/package-stage/windows-x64/native')
FRONTEND = Path('frontend/dist')
UX = Path('.build/target/debug/examples/ux_ui_probe.exe')
MANIFEST = 'manifest.json'
DIST = 'frontend-dist'


def ci_identity(env):
    commit, run, attempt = (env[key] for key in ('GITHUB_SHA', 'GITHUB_RUN_ID', 'GITHUB_RUN_ATTEMPT'))
    assert re.fullmatch(r'[0-9a-f]{40}', commit), 'invalid CI commit'
    assert all(re.fullmatch(r'[1-9][0-9]*', value) for value in (run, attempt)), 'invalid CI run or attempt'
    return dict(commit=commit, ciRun=run, ciAttempt=attempt)


def plain_stat(path):
    info = path.lstat()
    assert not stat.S_ISLNK(info.st_mode) and not (
        getattr(info, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
    ), f'linked WebView input: {path.name}'
    return info


def file_receipt(path):
    info = plain_stat(path)
    assert stat.S_ISREG(info.st_mode) and info.st_size > 0, f'invalid WebView input: {path.name}'
    with path.open('rb') as stream:
        return dict(bytes=info.st_size, sha256=hashlib.file_digest(stream, 'sha256').hexdigest())


def frontend_receipts(dist):
    assert stat.S_ISDIR(plain_stat(dist).st_mode), 'expected a frontend asset directory'
    files, seen = {}, set()
    pending = [dist]
    while pending:
        for path in sorted(pending.pop().iterdir()):
            relative = path.relative_to(dist).as_posix()
            key = relative.casefold()
            assert key not in seen, 'duplicate Windows frontend asset path'
            seen.add(key)
            assert len(seen) <= 4096, 'too many frontend assets'
            if stat.S_ISDIR(plain_stat(path).st_mode):
                pending.append(path)
            else:
                files[relative] = file_receipt(path)
    assert 'index.html' in files, 'missing frontend index'
    return files


def receipt(artifact, identity):
    return dict(schema=1, **identity, frontend=frontend_receipts(artifact / DIST),
                uxProbe=file_receipt(artifact / UX.name))


def stage(repo, identity):
    artifact = repo / ARTIFACT
    artifact.mkdir(parents=True)
    # Do not follow a symlink while staging; Actions transports plain files.
    files = frontend_receipts(repo / FRONTEND)
    dist = artifact / DIST
    for relative in files:
        path = dist / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(repo / FRONTEND / relative, path)
    ux = file_receipt(repo / UX)
    shutil.copyfile(repo / UX, artifact / UX.name)
    (artifact / MANIFEST).write_text(
        json.dumps(dict(schema=1, **identity, frontend=files, uxProbe=ux), sort_keys=True) + '\n',
        encoding='utf-8',
    )


def verify(artifact, identity):
    assert stat.S_ISDIR(plain_stat(artifact).st_mode), 'expected a WebView input artifact directory'
    assert {path.name for path in artifact.iterdir()} == {DIST, UX.name, MANIFEST}, 'unexpected WebView artifact contents'
    manifest = artifact / MANIFEST
    info = plain_stat(manifest)
    assert stat.S_ISREG(info.st_mode) and info.st_size <= 2 * 1024 * 1024, 'invalid WebView input manifest'
    stored = json.loads(manifest.read_text(encoding='utf-8'))
    assert stored == receipt(artifact, identity), 'WebView input provenance or checksum mismatch'


def restore(repo, identity, *, shared_runtime=None):
    artifact = repo / ARTIFACT
    verify(artifact, identity)
    dist, ux, native = (repo / path for path in (FRONTEND, UX, NATIVE))
    assert not dist.exists() and not dist.is_symlink(), 'expected a clean frontend dist destination'
    assert not native.exists() and not native.is_symlink(), 'expected a clean native runtime destination'
    native.parent.mkdir(parents=True, exist_ok=True)
    if shared_runtime is None:
        with tempfile.TemporaryDirectory(prefix='webview-runtime-', dir=native.parent) as temporary:
            portable = portable_runtime(
                repo / PACKAGES, identity['commit'], Path(temporary), ci_run=identity['ciRun'],
                ci_attempt=identity['ciAttempt'], require_package_flac=True,
            )
            portable.rename(native)
    else:
        from windows_release_runtime import restore as restore_runtime

        restore_runtime(shared_runtime, identity, native, include_notices=False)
    shutil.copytree(artifact / DIST, dist)
    ux.parent.mkdir(parents=True, exist_ok=True)
    # Use the verified shared executable, replacing any stale entry without following it.
    (artifact / UX.name).replace(ux)
    return native


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('stage', 'restore'))
    parser.add_argument('--shared-runtime', type=Path, help='use the verified same-attempt prepackage runtime')
    args = parser.parse_args()
    if args.operation == 'stage' and args.shared_runtime is not None:
        parser.error('--shared-runtime applies only to restore')
    repo, identity = Path.cwd(), ci_identity(os.environ)
    if args.operation == 'stage':
        stage(repo, identity)
    else:
        native = restore(repo, identity, shared_runtime=args.shared_runtime)
        with Path(os.environ['GITHUB_ENV']).open('a', encoding='utf-8') as stream:
            stream.write(f'MPV_DLL_DIR_CUMULUS={native}\n')
    print(json.dumps(dict(operation=args.operation, **identity)))


if __name__ == '__main__':
    main()
