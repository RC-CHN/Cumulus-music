"""Optional actual NSIS install, media layout, startup and uninstall diagnostic.

Run only in a disposable, offline Windows x64 user/VM with WebView2 installed.
Pass the packages-windows-x64 artifact from the explicitly identified CI run.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import wave

from windows_zip_smoke import PLATFORM, portable_runtime


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def plain(path, predicate):
    result = path.lstat()
    assert not getattr(result, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT, (
        f'unexpected installed reparse point: {path.name}')
    assert predicate(result.st_mode), f'unexpected installed file type: {path.name}'


def verified_installer(packages, commit, ci_run, ci_attempt):
    manifest = json.loads((packages / f'manifest-{PLATFORM}.json').read_text(encoding='utf-8'))
    assert (manifest['platform'], manifest['commit'], manifest['ciRun'], manifest['ciAttempt']) == (
        PLATFORM, commit, ci_run, ci_attempt), 'wrong installer provenance'
    version = manifest['version']
    assert re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version), 'invalid package version'
    name = f'Cumulus-{version}-{PLATFORM}-setup.exe'
    entries = [row for row in manifest['assets'] if row['name'] == name]
    assert len(entries) == 1, 'missing or duplicate NSIS installer manifest entry'
    setup = packages / name
    plain(setup, stat.S_ISREG)
    assert setup.stat().st_size == entries[0]['bytes'], 'NSIS installer size mismatch'
    assert digest(setup) == entries[0]['sha256'], 'NSIS installer checksum mismatch'
    return setup


def installed_media_matches(install, portable):
    plain(install, stat.S_ISDIR)
    expected = {file.name.lower(): file for file in portable.iterdir()}
    installed_dlls = {file.name.lower() for file in install.iterdir() if file.suffix.lower() == '.dll'}
    assert installed_dlls == {name for name in expected if name.endswith('.dll')}, (
        'installed DLL closure does not match the portable runtime')
    for name, copy in expected.items():
        installed = install / name
        plain(installed, stat.S_ISREG)
        assert installed.stat().st_size == copy.stat().st_size and digest(installed) == digest(copy), (
            f'installed file differs from portable runtime: {name}')
    return len(expected)


def nsis_command(program, install, *, remove=False):
    # NSIS interprets /D and _?= as the unquoted rest of the command line.
    # Passing a Python argv list would quote a destination containing spaces.
    assert program.is_absolute() and install.is_absolute()
    assert not any(char in str(install) for char in ('"', '\r', '\n', '\0'))
    directive = '_?=' if remove else '/D='
    return subprocess.list2cmdline([str(program), '/S']) + ' ' + directive + str(install)


def uninstall_entries():
    """Current-user uninstall metadata; never modify it directly."""
    import winreg

    namespace = r'Software\Microsoft\Windows\CurrentVersion\Uninstall'
    try:
        base = winreg.OpenKey(winreg.HKEY_CURRENT_USER, namespace)
    except FileNotFoundError:
        return []
    with base:
        result = []
        for index in range(winreg.QueryInfoKey(base)[0]):
            with winreg.OpenKey(base, winreg.EnumKey(base, index)) as item:
                row = {}
                for name in ('DisplayName', 'InstallLocation', 'UninstallString'):
                    try:
                        row[name] = str(winreg.QueryValueEx(item, name)[0])
                    except FileNotFoundError:
                        pass
                result.append(row)
        return result


def clean_environment():
    system = Path(os.environ['SystemRoot'])
    base = {key: value for key, value in os.environ.items()
            if not key.upper().startswith('CUMULUS_')
            and key.upper() not in {'PATH', 'MPV_DLL_DIR_CUMULUS'}}
    base['PATH'] = os.pathsep.join(map(str, (system / 'System32', system)))
    return base


def run_nsis(program, install, workdir, env, log, *, remove=False):
    with log.open('xb') as output:
        subprocess.run(nsis_command(program, install, remove=remove), executable=str(program),
                       cwd=workdir, env=env, check=True, timeout=180,
                       stdout=output, stderr=subprocess.STDOUT)


def verify_ffprobe(install, workdir, env):
    fixture = workdir / 'synthetic.wav'
    with wave.open(str(fixture), 'wb') as source:
        source.setparams((1, 2, 48000, 0, 'NONE', 'not compressed'))
        source.writeframes(bytes(4800 * 2))
    probe = subprocess.run([str(install / 'ffprobe.exe'), '-v', 'error', '-select_streams', 'a:0',
                            '-show_entries', 'stream=codec_name,sample_rate,channels',
                            '-of', 'json', str(fixture)], cwd=workdir, env=env,
                           check=True, capture_output=True, timeout=15)
    assert json.loads(probe.stdout)['streams'] == [
        {'codec_name': 'pcm_s16le', 'sample_rate': '48000', 'channels': 1}]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--packages', required=True, type=Path)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--ci-run', required=True)
    parser.add_argument('--ci-attempt', required=True)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--disposable-runner', required=True, action='store_true')
    args = parser.parse_args()
    assert os.name == 'nt', 'actual NSIS installation must run on Windows'
    assert re.fullmatch(r'[0-9a-f]{40}', args.commit), 'expected full commit SHA'
    assert all(re.fullmatch(r'[1-9][0-9]*', n) for n in (args.ci_run, args.ci_attempt)), (
        'expected positive source CI run and attempt')
    assert not any(row.get('DisplayName', '').casefold() == 'cumulus' for row in uninstall_entries()), (
        'existing Cumulus installation in current user; use a disposable runner')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    packages = args.packages.resolve(strict=True)
    setup = verified_installer(packages, args.commit, args.ci_run, args.ci_attempt)
    env = clean_environment()
    graphics = Path(__file__).with_name('graphics_smoke.py').resolve()

    with tempfile.TemporaryDirectory(prefix='cumulus-nsis-diagnostic-') as folder:
        scratch = Path(folder).resolve(strict=True)
        workdir = scratch / 'different working directory'
        workdir.mkdir()
        install = scratch / 'installed application'
        portable = portable_runtime(packages, args.commit, scratch,
                                    ci_run=args.ci_run, ci_attempt=args.ci_attempt)
        try:
            run_nsis(setup, install, workdir, env, root / 'install.log')
            count = installed_media_matches(install, portable)
            verify_ffprobe(install, workdir, env)
            subprocess.run([sys.executable, str(graphics), '--binary', str(install / 'cumulus.exe'),
                            '--root', str(root / 'graphics')], cwd=workdir, env=env, check=True,
                           timeout=240)
        finally:
            # Run the generated uninstaller in place so the parent observes
            # completion. NSIS may leave that running executable behind.
            if os.path.lexists(install):
                plain(install, stat.S_ISDIR)
                uninstaller = install / 'uninstall.exe'
                plain(uninstaller, stat.S_ISREG)
                run_nsis(uninstaller, install, workdir, env, root / 'uninstall.log', remove=True)
                assert not any(os.path.lexists(install / p.name) for p in portable.iterdir()), (
                    'uninstaller left installed runtime files')
            assert not any(str(install).casefold() in ' '.join(row.values()).casefold()
                           or row.get('DisplayName', '').casefold() == 'cumulus'
                           for row in uninstall_entries()), 'uninstaller left current-user registration'
    report = dict(ok=True, commit=args.commit, ciRun=args.ci_run, ciAttempt=args.ci_attempt,
                  actualNsisInstall=True, matchedRuntimeFiles=count, installedFfprobe=True,
                  installedGraphicsDiagnostic=True, uninstallVerified=True,
                  installedPlaybackVerified=False, physicalAudioOutputVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
