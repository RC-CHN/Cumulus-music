"""Run held-source native Rust tests with the staged Windows media DLLs."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from cargo_core_test_harness import build_core_test_executable
from windows_media import download, sha256, write_json
from windows_flac import (
    FLAC, FLAC_ARCHIVE, FLAC_ARCHIVE_SHA256, FLAC_DLL_SHA256, FLAC_IMPORTS,
    FLAC_PACKAGE, FLAC_RELEASE, FLAC_URL, flac_imports,
)

ROOT = Path(__file__).resolve().parents[2]
MPV = 'libmpv-2.dll'
RECEIPT = 'CUMULUS_WINDOWS_COLD_FLAC_RUNTIME '

ENGINE = 'playback::engine'
DECODER = f'{ENGINE}::limiter_segment::inprocess_flac'
COLD = f'{DECODER}::cold_authority'
STOP = f'{ENGINE}::mpv::pinned_stop::tests'
IDENTITY = f'{DECODER}::windows_runtime::limiter_windows_cold_flac_runtime_identity'
ROOTED_WAV_EQUALIZER = ('library::native_open::windows::tests::'
                        'windows_rooted_actor_loads_held_wav_equalizer_after_filename_replacement')
INDEXED_WAV = tuple(
    f'library::index::held_audio::windows_tests::{case}'
    for case in (
        'windows_indexed_wave_keeps_scan_identity_and_refuses_a_legacy_or_replaced_path',
        'windows_indexed_wave_does_not_adopt_a_late_download_root_alias',
    )
)
CASES = (
    IDENTITY,
    f'{COLD}::stereo::cold_stereo_unknown_length_native_pause_controls_seek_tail_and_revocation',
    f'{DECODER}::limiter_inprocess_flac_exact_random_seek_and_generation_eof',
    f'{COLD}::limiter_inprocess_flac_cold_grant_retains_open_file_and_rejects_buffered_pcm_after_detach',
    f'{COLD}::sink::limiter_cold_flac_sink_seek_eof_and_buffered_revocation',
    f'{COLD}::fallback::limiter_cold_flac_fallback_preserves_the_held_file_and_native_transport',
    f'{COLD}::fallback::limiter_cold_flac_fallback_does_not_publish_bad_position_or_revoked_source',
    f'{COLD}::sink::handoff::limiter_cold_flac_active_seek_retires_sink_and_reclaims_exact_source',
    f'{COLD}::production::limiter_cold_flac_production_controller_opt_in_and_stock_fallback',
    f'{STOP}::pinned_stop_native_rejection_and_stale_ack_both_destroy_the_sink',
    f'{STOP}::pinned_stop_revocation_while_opening_closes_the_unpublished_sink',
    ROOTED_WAV_EQUALIZER,
    *INDEXED_WAV,
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


def stage_runtime(package, msys, working, *, require_package_flac=False):
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
    if require_package_flac and FLAC.lower() not in dlls:
        raise ValueError('packaged libFLAC is required; refusing a proof-only replacement')
    origin = ensure_flac(runtime, msys, working)
    if require_package_flac and origin != 'package-stage':
        raise ValueError('native verification did not use the packaged libFLAC DLL')
    imports = flac_imports(runtime / FLAC, msys / 'ucrt64/bin/objdump.exe')
    dlls[FLAC.lower()] = sha256(runtime / FLAC)
    if dlls[FLAC.lower()] != FLAC_DLL_SHA256:
        raise ValueError('staged libFLAC bytes changed before native verification')
    return runtime, origin, dlls, sorted(imports)


def test_executable(log, *, repo=ROOT):
    return build_core_test_executable(log, repo=repo)


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


def verify(root, package, msys, native_audio_probe=None, *, require_package_flac=False,
           core_test_binary=None):
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
        'coreTestBinary': 'prebuilt' if core_test_binary is not None else 'local-cargo',
        'passed': [],
    }
    try:
        with tempfile.TemporaryDirectory(prefix='cold-flac-runtime-', dir=root.parent) as directory, \
                (root / 'tests.log').open('x', encoding='utf-8') as log:
            runtime, origin, dlls, imports = stage_runtime(
                package, msys, Path(directory), require_package_flac=require_package_flac,
            )
            report.update(libflacOrigin=origin, libflacPackaged=origin == 'package-stage',
                          dllSha256=dlls, libflacImports=imports)
            test = core_test_binary if core_test_binary is not None else test_executable(log)
            system = Path(os.environ['SystemRoot'])
            excluded = {'PATH', 'MPV_DLL_DIR_CUMULUS', 'CUMULUS_FLAC_PROBE_LIBRARY',
                        'CUMULUS_LIMITER_PROBE_MPV'}
            env = {key: value for key, value in os.environ.items() if key.upper() not in excluded}
            env.update(
                PATH=os.pathsep.join(map(str, (system / 'System32', system))),
                CUMULUS_FLAC_PROBE_LIBRARY=str(runtime / FLAC),
                CUMULUS_LIMITER_PROBE_MPV=str(runtime / MPV),
            )
            # Exercise the actual fixed NTFS guard on this runner's local
            # checkout volume. The tests do not skip if it is unsupported.
            indexed_scratch = Path(directory) / 'indexed-wav-fixtures'
            indexed_scratch.mkdir()
            indexed_env = {key: value for key, value in env.items()
                           if key.upper() not in ('TEMP', 'TMP')}
            indexed_env.update(TEMP=str(indexed_scratch), TMP=str(indexed_scratch))
            for case in CASES:
                result = subprocess.run(
                    [test, '--exact', case, '--include-ignored', '--nocapture', '--color', 'never'],
                    cwd=runtime, env=indexed_env if case in INDEXED_WAV else env,
                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
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
            if native_audio_probe is not None:
                gui_report = root / 'stereo-pcm-gui.json'
                subprocess.run(
                    [sys.executable, ROOT / 'scripts/ci/native_smoke.py',
                     '--binary', native_audio_probe, '--report', gui_report,
                     '--runtime', runtime, '--stereo-pcm-flac-library', runtime / FLAC,
                     '--mpv-library', runtime / MPV],
                    cwd=ROOT, env=env, check=True, timeout=120,
                )
                gui = json.loads(gui_report.read_text(encoding='utf-8'))
                observed = gui['nativeRuntime']
                if (observed['kind'] != 'explicit_windows_runtime'
                        or not os.path.samefile(observed['libmpvModule'], runtime / MPV)):
                    raise ValueError('GUI did not use the verified temporary libmpv')
                report['stereoPcmGui'] = {'report': gui_report.name, 'ok': True,
                                         'nativeRuntime': observed}
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
    parser.add_argument('--native-audio-probe', type=Path, help='same-build GUI probe inside the verified temporary runtime')
    parser.add_argument('--core-test-binary', type=Path,
                        help='use the core libtest harness restored from this CI attempt instead of compiling it')
    parser.add_argument('--require-package-flac', action='store_true', help='reject missing packaged FLAC rather than filling the private proof directory')
    args = parser.parse_args()
    if sys.platform != 'win32':
        parser.error('run native verification on Windows')
    # Rust probe diagnostics are UTF-8. A redirected Windows stdout may use
    # CP1252, which cannot print some successful-case diagnostics.
    sys.stdout.reconfigure(encoding='utf-8')
    verify(args.report.resolve(), args.runtime.resolve(strict=True),
           Path(os.environ['CUMULUS_MSYS2_ROOT']).resolve(strict=True),
           args.native_audio_probe.resolve(strict=True) if args.native_audio_probe else None,
           require_package_flac=args.require_package_flac,
           core_test_binary=args.core_test_binary.resolve(strict=True) if args.core_test_binary else None)


if __name__ == '__main__':
    main()
