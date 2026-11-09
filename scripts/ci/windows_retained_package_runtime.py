"""Replay the three package-runtime checks from one retained Windows trial."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


PRODUCER = dict(commit='2d913f04b4535efb6cb942305ed82cf98ed60455', ciRun='42686828896', ciAttempt='1')
ARTIFACTS = dict(package=13707187497, frontend=13704927653, inputs=13704973823, probes=13705759388)
MANIFESTS = {
    '.build/packages/windows-x64/manifest-windows-x64.json':
        '52f371dd37700cb99e914de0adb9408c01372bb61199f0772da1d53beaea66c8',
    '.build/windows-webview-inputs/manifest.json':
        'd3dc482ca24e466a4cc6185433dbf77d8c83664b5976b26acbb0196f3e775505',
    '.build/windows-webview-probes/package-runtime/manifest.json':
        '87e593eae5e88cc436856c15858ede1438aea135c6ef8b588291d26e870696c8',
}


def restore(producer):
    def git(*args):
        return subprocess.check_output(['git', *args], cwd=producer, text=True).strip()

    assert git('rev-parse', 'HEAD') == PRODUCER['commit'], 'wrong producer source'
    assert not git('status', '--porcelain', '--untracked-files=normal'), 'producer checkout is not clean'
    sys.path.insert(0, str(producer / 'scripts/ci'))
    import windows_webview_inputs as inputs
    import windows_webview_probes as probes

    for relative, expected in MANIFESTS.items():
        assert inputs.file_receipt(producer / relative)['sha256'] == expected, f'wrong manifest: {relative}'
    original_frontend = inputs.frontend_receipts(producer / '.build/retained-frontend-dist')
    assert original_frontend == inputs.frontend_receipts(producer / inputs.ARTIFACT / inputs.DIST)
    native = inputs.restore(producer, PRODUCER)
    probes.restore(producer, PRODUCER, 'package-runtime')
    return native, inputs


def commands():
    runtime = '.build/package-stage/windows-x64/native'
    examples = '.build/target/debug/examples'
    return (
        ('cold-flac', ['scripts/ci/windows_cold_flac.py', '--runtime', runtime,
                       '--report', '.build/ci-windows-cold-flac', '--native-audio-probe',
                       f'{examples}/native_audio_probe.exe', '--core-test-binary',
                       f'{examples}/cumulus_core_cold_flac_tests.exe', '--require-package-flac']),
        ('portable-zip', ['scripts/ci/windows_zip_smoke.py', '--packages', '.build/packages/windows-x64',
                          '--commit', PRODUCER['commit'], '--root', '.build/ci-windows-zip']),
        ('local-queue', ['scripts/ci/local_queue_smoke.py', '--binary', f'{examples}/local_ui_probe.exe',
                         '--root', '.build/ci-local-queue-windows', '--runtime', runtime]),
    )


def replay(producer, report_path):
    assert sys.platform == 'win32' and sys.maxsize > 2**32, 'requires native Windows x64 Python'
    assert tuple(os.environ.get(key) for key in ('GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH')) == (
        'true', 'github-hosted', 'Windows', 'X64',
    ), 'requires a disposable GitHub-hosted Windows x64 runner'
    report_path.parent.mkdir(parents=True, exist_ok=False)
    report = dict(ok=False, producer=PRODUCER, artifacts=ARTIFACTS,
                  proofCi={field: os.environ[key] for field, key in (
                      ('commit', 'GITHUB_SHA'), ('ciRun', 'GITHUB_RUN_ID'), ('ciAttempt', 'GITHUB_RUN_ATTEMPT'),
                  )}, passed=[], failureStage='retained-inputs')
    try:
        native, inputs = restore(producer)
        report['failureStage'] = 'separate-proof-tools'
        msys = Path(os.environ['CUMULUS_MSYS2_ROOT']).resolve(strict=True)
        ffmpeg = (msys / 'ucrt64/bin/ffmpeg.exe').resolve(strict=True)
        objdump = (msys / 'ucrt64/bin/objdump.exe').resolve(strict=True)
        assert ffmpeg == Path(os.environ['CUMULUS_LOCAL_FIXTURE_FFMPEG']).resolve(strict=True)
        assert not msys.is_relative_to(producer) and not producer.is_relative_to(msys)
        report['proofTools'] = dict(ffmpeg=inputs.file_receipt(ffmpeg), objdump=inputs.file_receipt(objdump))
        report['portableApp'] = inputs.file_receipt(native / 'cumulus.exe')
        # Only children see producer IDs for their existing package validation.
        # MSYS2 is not a fallback search directory for any of the package DLLs.
        system = Path(os.environ['SystemRoot']).resolve(strict=True)
        env = {key: value for key, value in os.environ.items()
               if not key.upper().startswith('CUMULUS_') and key.upper() not in {'PATH', 'MPV_DLL_DIR_CUMULUS'}}
        env.update(GITHUB_SHA=PRODUCER['commit'], GITHUB_RUN_ID=PRODUCER['ciRun'],
                   GITHUB_RUN_ATTEMPT=PRODUCER['ciAttempt'], CUMULUS_MSYS2_ROOT=str(msys),
                   CUMULUS_LOCAL_FIXTURE_FFMPEG=str(ffmpeg), MPV_DLL_DIR_CUMULUS=str(native),
                   PATH=os.pathsep.join(map(str, (system / 'System32', system))))
        for name, argv in commands():
            report['failureStage'] = name
            subprocess.run([sys.executable, *argv], cwd=producer, env=env, check=True, timeout=420)
            report['passed'].append(name)
        report.update(ok=True, failureStage=None)
    finally:
        with report_path.open('x', encoding='utf-8') as stream:
            stream.write(json.dumps(report, indent=2, sort_keys=True) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('verify-inputs', 'replay'))
    parser.add_argument('--producer', type=Path, required=True)
    parser.add_argument('--report', type=Path)
    args = parser.parse_args()
    producer = args.producer.resolve(strict=True)
    if args.operation == 'verify-inputs':
        native, _ = restore(producer)
        print(json.dumps(dict(ok=True, producer=PRODUCER, verifiedRuntimeFiles=len(list(native.iterdir())))))
    elif args.report is None:
        parser.error('replay needs a report path')
    else:
        replay(producer, args.report.resolve())


if __name__ == '__main__':
    main()
