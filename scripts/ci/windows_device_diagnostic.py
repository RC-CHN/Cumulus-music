"""Windows synthetic media regression, without waiting for an NSIS build.

Pass --legacy-property-probes to also reproduce the unused upstream property
API fault. Any selected probe failure fails this command; none are ignored.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys
from package import windows_media


if __name__ == '__main__':
    assert sys.platform == 'win32'
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--legacy-property-probes', action='store_true')
    args = parser.parse_args()
    root = Path('.build/windows-device-diagnostic').resolve()
    root.mkdir(parents=True, exist_ok=False)
    native = root / 'runtime'
    native.mkdir()
    msys = Path(os.environ['CUMULUS_MSYS2_ROOT'])
    windows_media(native, msys)
    executable = native / 'engine_roundtrip.exe'
    shutil.copy2(Path(os.environ.get('CARGO_TARGET_DIR', '.build/target')) / 'debug/examples/engine_roundtrip.exe', executable)
    env = os.environ.copy()
    # os.environ is case-insensitive on Windows; its dict copy is not.
    system = Path(os.environ['SystemRoot'])
    env['PATH'] = os.pathsep.join(map(str, [native, system / 'System32', system]))
    failures = []
    modes = ['single-sync', 'pair-sync', 'single-command', 'pair-command']
    if args.legacy_property_probes:
        modes.extend(['single-async', 'pair-async', 'pair-async-drain'])
    commands = [
        ('rust', [str(executable)]),
        *[(mode, [sys.executable, 'scripts/ci/windows_device_ctypes.py', str(native), mode])
          for mode in modes],
        ('upstream', [str(msys / 'ucrt64/bin/mpv.exe'), '--no-config', '--audio-device=help']),
    ]
    for name, command in commands:
        with (root / f'{name}.log').open('x', encoding='utf-8') as log:
            try:
                result = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=60)
                code = result.returncode
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(str(error)); code = -1
        print(f'{name}: exit {code}', flush=True)
        print((root / f'{name}.log').read_text(encoding='utf-8', errors='replace'), flush=True)
        if code:
            failures.append(name)
    if failures:
        raise SystemExit('Native diagnostic failures: ' + ', '.join(failures))
