"""Run the opted-in cold FLAC Rust tests with the staged Windows media DLLs."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile

from windows_media import download, sha256, write_json

ROOT = Path(__file__).resolve().parents[2]
FLAC = 'libFLAC.dll'
MPV = 'libmpv-2.dll'
FLAC_PACKAGE = 'mingw-w64-ucrt-x86_64-flac'
FLAC_RELEASE = '1.5.0-2'
FLAC_ARCHIVE = f'{FLAC_PACKAGE}-{FLAC_RELEASE}-any.pkg.tar.zst'
FLAC_URL = f'https://repo.msys2.org/mingw/ucrt64/{FLAC_ARCHIVE}'
FLAC_ARCHIVE_SHA256 = '28e8637795bd7d80289ddbb582d6ad350b73eac64390b877149717dd3757ad79'
FLAC_DLL_SHA256 = 'c01d2adbb5f3a09c27bb6a857553d2f2143293b6e07aab0ac0b2a203337e54eb'
FLAC_IMPORTS = {'libogg-0.dll', 'libwinpthread-1.dll'}
RECEIPT = 'CUMULUS_WINDOWS_COLD_FLAC_RUNTIME '

ENGINE = 'playback::engine'
DECODER = f'{ENGINE}::limiter_segment::inprocess_flac'
COLD = f'{DECODER}::cold_authority'
STOP = f'{ENGINE}::mpv::pinned_stop::tests'
IDENTITY = f'{DECODER}::windows_runtime::limiter_windows_cold_flac_runtime_identity'
CASES = (
    IDENTITY,
    f'{DECODER}::limiter_inprocess_flac_exact_random_seek_and_generation_eof',
    f'{COLD}::limiter_inprocess_flac_cold_grant_retains_open_file_and_rejects_buffered_pcm_after_detach',
    f'{COLD}::sink::limiter_cold_flac_sink_seek_eof_and_buffered_revocation',
    f'{COLD}::fallback::limiter_cold_flac_fallback_preserves_the_held_file_and_native_transport',
    f'{COLD}::fallback::limiter_cold_flac_fallback_does_not_publish_bad_position_or_revoked_source',
    f'{COLD}::sink::handoff::limiter_cold_flac_active_seek_retires_sink_and_reclaims_exact_source',
    f'{STOP}::pinned_stop_native_rejection_and_stale_ack_both_destroy_the_sink',
    f'{STOP}::pinned_stop_revocation_while_opening_closes_the_unpublished_sink',
)


def ensure_flac(runtime, msys, working):
    """Never repair a mismatched package DLL in place or load an ambient one."""
    target = runtime / FLAC
    if target.exists():
        if sha256(target) != FLAC_DLL_SHA256:
            raise ValueError('package-stage libFLAC does not match the audited Windows DLL')
        return 'package-stage'
    installed = msys / 'ucrt64/bin' / FLAC
    if installed.is_file() and sha256(installed) == FLAC_DLL_SHA256:
        shutil.copyfile(installed, target)
        return 'msys2-ucrt64-installed'
    archive = working / FLAC_ARCHIVE
    download(FLAC_URL, archive, FLAC_ARCHIVE_SHA256)
    # Select the one named member as bytes; do not unpack archive paths or
    # install a new package into the runner's shipping MSYS2 environment.
    msys_archive = subprocess.check_output(
        [msys / 'usr/bin/cygpath.exe', '-u', archive], encoding='utf-8', timeout=10,
    ).strip()
    binary = subprocess.check_output(
        [msys / 'usr/bin/bsdtar.exe', '-xOf', msys_archive, f'ucrt64/bin/{FLAC}'],
        timeout=20,
    )
    target.write_bytes(binary)
    if sha256(target) != FLAC_DLL_SHA256:
        raise ValueError('pinned MSYS2 archive did not contain the audited libFLAC DLL')
    return 'pinned-msys2-archive'


def stage_runtime(package, msys, working):
    runtime = working / 'runtime'
    runtime.mkdir()
    dlls = {}
    for source in sorted(package.iterdir()):
        if source.is_file() and source.suffix.lower() == '.dll':
            name = source.name.lower()
            if name in dlls or source.is_symlink():
                raise ValueError(f'ambiguous or linked package runtime file: {source.name}')
            destination = runtime / source.name
            shutil.copyfile(source, destination)
            dlls[name] = sha256(destination)
    if MPV not in dlls or not FLAC_IMPORTS <= dlls.keys():
        raise ValueError('missing libmpv or libFLAC non-system imports in package runtime')
    origin = ensure_flac(runtime, msys, working)
    objdump = msys / 'ucrt64/bin/objdump.exe'
    metadata = subprocess.check_output(
        [objdump, '-p', runtime / FLAC], encoding='utf-8', errors='replace', timeout=20,
    )
    imports = {name.lower() for name in re.findall(r'DLL Name:\s*(\S+)', metadata)}
    non_system = {
        name for name in imports
        if not name.startswith(('api-ms-', 'ext-ms-')) and name != 'kernel32.dll'
    }
    if 'file format pei-x86-64' not in metadata or non_system != FLAC_IMPORTS:
        raise ValueError('unexpected architecture or imports for the pinned libFLAC DLL')
    dlls[FLAC.lower()] = sha256(runtime / FLAC)
    if dlls[FLAC.lower()] != FLAC_DLL_SHA256:
        raise ValueError('staged libFLAC bytes changed before native verification')
    return runtime, origin, dlls, sorted(imports)


def test_executable(log):
    args = ['cargo', 'test', '--locked', '-p', 'cumulus-core', '--lib', '--no-run',
            '--message-format=json']
    result = subprocess.run(args, cwd=ROOT, capture_output=True, encoding='utf-8',
                            errors='replace', check=False, timeout=600)
    log.write(result.stderr)
    if result.returncode:
        log.write(result.stdout)
        raise RuntimeError(f'could not build cold FLAC Rust tests (exit {result.returncode})')
    log.flush()
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    paths = {
        (ROOT / item['executable']).resolve(strict=True)
        for item in artifacts
        if item.get('reason') == 'compiler-artifact' and item.get('executable')
        and item['target']['name'] == 'cumulus_core' and item['profile']['test']
    }
    if len(paths) != 1:
        raise ValueError('expected exactly one cargo-produced core test executable')
    return paths.pop()


def case_result(result, case):
    output = result.stdout
    if (result.returncode or '1 passed; 0 failed; 0 ignored' not in output
            or f'test {case} ... ok' not in output):
        raise RuntimeError(f'Windows native cold FLAC case did not pass exactly once: {case}')
    if case == IDENTITY:
        receipts = [line.removeprefix(RECEIPT) for line in output.splitlines()
                    if line.startswith(RECEIPT)]
        if len(receipts) != 1:
            raise ValueError('Windows native cold FLAC module identity receipt is missing')
        return json.loads(receipts[0])
    return None


def check_identity(receipt, runtime):
    if (receipt.get('libflacVersion') != '1.5.0'
            or type(receipt.get('libmpvApi')) is not int or receipt['libmpvApi'] <= 0
            or not receipt.get('libmpvVersion')
            or receipt.get('relativeLookupRejected') is not True
            or not os.path.samefile(receipt['libflacModule'], runtime / FLAC)
            or not os.path.samefile(receipt['libmpvModule'], runtime / MPV)):
        raise ValueError('Windows native receipt does not identify the verified runtime')


def verify(root, package, msys):
    root.mkdir(parents=True, exist_ok=False)
    report = {
        'ok': False,
        'commit': os.environ.get('GITHUB_SHA'),
        'ciRun': os.environ.get('GITHUB_RUN_ID'),
        'ciAttempt': os.environ.get('GITHUB_RUN_ATTEMPT'),
        'libflacPackage': f'{FLAC_PACKAGE} {FLAC_RELEASE}',
        'libflacArchiveUrl': FLAC_URL,
        'libflacArchiveSha256': FLAC_ARCHIVE_SHA256,
        'audioOutput': 'null',
        'optionalLimiterShipped': False,
        'passed': [],
    }
    try:
        with tempfile.TemporaryDirectory(prefix='cold-flac-runtime-', dir=root.parent) as directory, \
                (root / 'tests.log').open('x', encoding='utf-8') as log:
            runtime, origin, dlls, imports = stage_runtime(package, msys, Path(directory))
            report.update(libflacOrigin=origin, dllSha256=dlls, libflacImports=imports)
            test = test_executable(log)
            system = Path(os.environ['SystemRoot'])
            excluded = {'PATH', 'MPV_DLL_DIR_CUMULUS', 'CUMULUS_FLAC_PROBE_LIBRARY',
                        'CUMULUS_LIMITER_PROBE_MPV'}
            env = {key: value for key, value in os.environ.items() if key.upper() not in excluded}
            env.update(
                PATH=os.pathsep.join(map(str, (system / 'System32', system))),
                CUMULUS_FLAC_PROBE_LIBRARY=str(runtime / FLAC),
                CUMULUS_LIMITER_PROBE_MPV=str(runtime / MPV),
            )
            for case in CASES:
                result = subprocess.run(
                    [test, '--exact', case, '--include-ignored', '--nocapture', '--color', 'never'],
                    cwd=runtime, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                    encoding='utf-8', errors='replace', check=False, timeout=45,
                )
                log.write(result.stdout)
                log.flush()
                print(result.stdout, end='', flush=True)
                receipt = case_result(result, case)
                if receipt is not None:
                    check_identity(receipt, runtime)
                    report['nativeRuntime'] = receipt
                report['passed'].append(case)
            if any(sha256(runtime / name) != digest for name, digest in dlls.items()):
                raise ValueError('staged DLL bytes changed during verification')
        report['ok'] = True
    except Exception as error:
        report['failure'] = str(error)
        raise
    finally:
        write_json(root / 'report.json', report)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', required=True, type=Path)
    parser.add_argument('--report', required=True, type=Path)
    args = parser.parse_args()
    if sys.platform != 'win32':
        parser.error('run native verification on Windows')
    verify(args.report.resolve(), args.runtime.resolve(strict=True),
           Path(os.environ['CUMULUS_MSYS2_ROOT']).resolve(strict=True))


if __name__ == '__main__':
    main()
