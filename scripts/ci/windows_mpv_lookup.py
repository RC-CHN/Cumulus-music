"""Ensure native Windows playback never implicitly trusts PATH or CWD."""
import os
from pathlib import Path
import subprocess
import sys


def main():
    assert sys.platform == 'win32'
    name = 'MPV_DLL_DIR_CUMULUS'
    runtime = Path(os.environ[name]).resolve(strict=True)
    assert (runtime / 'libmpv-2.dll').is_file(), 'positive runtime fixture missing'
    target = Path(os.environ.get('CARGO_TARGET_DIR', '.build/target')).resolve(strict=True)
    probe = target / 'debug/examples/player_probe.exe'
    plugin = target / 'debug/cumulus_audio_filter.dll'
    assert probe.is_file() and plugin.is_file()
    assert not any((probe.parent / dll).exists() for dll in ('libmpv-2.dll', 'mpv-2.dll'))
    proof = Path('.build/windows-media/untrusted-sdk-proof').resolve()
    env = {key: value for key, value in os.environ.items() if key.upper() not in (name, 'PATH')}
    env['PATH'] = str(runtime) + os.pathsep + os.environ.get('PATH', '')
    result = subprocess.run(
        [str(probe), str(plugin), str(proof)], cwd=runtime, env=env,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8',
        timeout=30, check=False)
    proof.with_suffix('.log').write_text(result.stderr, encoding='utf-8')
    assert result.returncode == 1 and 'library unavailable' in result.stderr, (
        f'untrusted Windows libmpv lookup did not fail as expected: {result.returncode}')


if __name__ == '__main__':
    main()
