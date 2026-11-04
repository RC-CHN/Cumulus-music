"""Optional actual NSIS install, media layout, startup and uninstall diagnostic.

Run only in a disposable Windows x64 user/VM with WebView2 already installed.
The hosted runner preflight is not network isolation.
Pass the packages-windows-x64 artifact from the explicitly identified CI run.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PureWindowsPath
import platform
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


UNINSTALL_KEY = r'Software\Microsoft\Windows\CurrentVersion\Uninstall'
PRODUCT_KEY = r'Software\Cumulus Music\Cumulus'
WEBVIEW2_CLIENT = r'Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'


def uninstall_entries(root):
    """Read both views; HKCU views may return the same registration twice."""
    import winreg

    seen = set()
    result = []
    for view in (winreg.KEY_WOW64_32KEY, winreg.KEY_WOW64_64KEY):
        try:
            base = winreg.OpenKey(root, UNINSTALL_KEY, 0, winreg.KEY_READ | view)
        except FileNotFoundError:
            continue
        with base:
            for index in range(winreg.QueryInfoKey(base)[0]):
                key = winreg.EnumKey(base, index)
                with winreg.OpenKey(base, key, 0, winreg.KEY_READ | view) as item:
                    row = {'Key': key}
                    for name in ('DisplayName', 'DisplayVersion', 'InstallLocation', 'UninstallString'):
                        try:
                            row[name] = str(winreg.QueryValueEx(item, name)[0])
                        except FileNotFoundError:
                            pass
                identity = tuple(row.items())
                if identity not in seen:
                    seen.add(identity)
                    result.append(row)
    return result


def cumulus_entries(entries):
    return [row for row in entries if any(
        row.get(key, '').casefold() == 'cumulus' for key in ('Key', 'DisplayName'))]


def registry_value(root, path, name):
    import winreg

    try:
        with winreg.OpenKey(root, path, 0, winreg.KEY_READ | winreg.KEY_WOW64_64KEY) as key:
            value, value_type = winreg.QueryValueEx(key, name)
    except FileNotFoundError:
        return None
    assert value_type == winreg.REG_SZ and isinstance(value, str), 'invalid WebView2 registry value'
    return value


def existing_webview_version(machine, user):
    # Match the shipped x64 NSIS order: a nonempty machine value takes
    # precedence. Do not accept malformed/zero values as installed.
    version = machine or user or ''
    assert re.fullmatch(r'[0-9]{1,8}(?:\.[0-9]{1,8}){3}', version) and any(
        int(part) for part in version.split('.')), 'WebView2 Evergreen Runtime is not preinstalled'
    return version


def preflight(hosted, environ):
    import winreg

    assert platform.machine().lower() in {'amd64', 'x86_64'} and sys.maxsize > 2**32, (
        'the installer diagnostic requires Windows x64 and x64 Python')
    if hosted:
        assert tuple(environ.get(key) for key in (
            'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH')) == (
            'github-hosted', 'Windows', 'X64'), 'a disposable GitHub-hosted Windows x64 runner is required'
    for root in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
        assert not cumulus_entries(uninstall_entries(root)), 'existing Cumulus uninstall registration'
        for view in (winreg.KEY_WOW64_32KEY, winreg.KEY_WOW64_64KEY):
            try:
                with winreg.OpenKey(root, PRODUCT_KEY, 0, winreg.KEY_READ | view):
                    raise AssertionError('existing Cumulus product registration')
            except FileNotFoundError:
                pass
    for variable, child in (('LOCALAPPDATA', 'Cumulus'), ('LOCALAPPDATA', 'io.cumulus.music'),
                            ('APPDATA', 'io.cumulus.music')):
        parent = Path(environ[variable])
        assert parent.is_absolute() and parent.is_dir(), 'missing Windows user data directory'
        assert not os.path.lexists(parent / child), 'existing Cumulus installation or user data'
    machine = registry_value(winreg.HKEY_LOCAL_MACHINE, rf'SOFTWARE\WOW6432Node\{WEBVIEW2_CLIENT}', 'pv')
    user = registry_value(winreg.HKEY_CURRENT_USER, rf'Software\{WEBVIEW2_CLIENT}', 'pv') if not machine else None
    return existing_webview_version(machine, user)


def quoted_windows_path(value):
    if len(value) < 3 or not value.startswith('"') or not value.endswith('"') or '"' in value[1:-1]:
        return None
    path = PureWindowsPath(value[1:-1])
    return path if path.is_absolute() else None


def require_scratch_registration(entries, install, version):
    matches = cumulus_entries(entries)
    assert len(matches) == 1, 'missing or ambiguous current-user scratch registration'
    row, = matches
    directory = PureWindowsPath(install)
    assert (row.get('Key', '').casefold(), row.get('DisplayName'), row.get('DisplayVersion'),
            quoted_windows_path(row.get('InstallLocation', '')),
            quoted_windows_path(row.get('UninstallString', ''))) == (
        'cumulus', 'Cumulus', version, directory, directory / 'uninstall.exe'), (
        'uninstall registration does not identify the exact scratch installation')


def clean_environment(environ, temporary):
    system = Path(environ['SystemRoot'])
    assert system.is_absolute() and system.is_dir(), 'missing Windows system directory'
    base = {key: value for key, value in environ.items()
            if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_', 'ACTIONS_ID_TOKEN_'))
            and key.upper() not in {'PATH', 'MPV_DLL_DIR_CUMULUS', 'TEMP', 'TMP',
                                    'GH_TOKEN', 'GITHUB_TOKEN', 'ACTIONS_RUNTIME_TOKEN'}}
    base['PATH'] = os.pathsep.join(map(str, (system / 'System32', system)))
    base['TEMP'] = base['TMP'] = str(temporary)
    base['PYTHONUTF8'] = '1'
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
    parser.add_argument('--hosted-runner', action='store_true', help='also require GitHub-hosted Windows x64')
    args = parser.parse_args()
    if not __debug__:
        raise RuntimeError('installer verification assertions must be enabled')
    assert os.name == 'nt', 'actual NSIS installation must run on Windows'
    assert re.fullmatch(r'[0-9a-f]{40}', args.commit), 'expected full commit SHA'
    assert all(re.fullmatch(r'[1-9][0-9]*', n) for n in (args.ci_run, args.ci_attempt)), (
        'expected positive source CI run and attempt')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    report = dict(ok=False, commit=args.commit, ciRun=args.ci_run, ciAttempt=args.ci_attempt,
                  hostedRunnerPreflight=False, networkIsolationVerified=False,
                  scratchRegistrationVerified=False,
                  actualNsisInstall=False, installedFfprobe=False,
                  installedGraphicsDiagnostic=False, uninstallVerified=False,
                  installedPlaybackVerified=False, physicalAudioOutputVerified=False)
    stage = 'preflight'
    try:
        import winreg

        report['webview2Runtime'] = preflight(args.hosted_runner, os.environ)
        report['hostedRunnerPreflight'] = args.hosted_runner
        stage = 'package'
        packages = args.packages.resolve(strict=True)
        setup = verified_installer(packages, args.commit, args.ci_run, args.ci_attempt)
        version = json.loads((packages / f'manifest-{PLATFORM}.json').read_text(encoding='utf-8'))['version']
        graphics = Path(__file__).with_name('graphics_smoke.py').resolve()
        with tempfile.TemporaryDirectory(prefix='cumulus-nsis-diagnostic-', dir=root.parent) as folder:
            scratch = Path(folder).resolve(strict=True)
            workdir = scratch / 'different working directory'
            workdir.mkdir()
            temp = scratch / 'process temporary files'
            temp.mkdir()
            env = clean_environment(os.environ, temp)
            install = scratch / 'installed application'
            portable = portable_runtime(packages, args.commit, scratch,
                                        ci_run=args.ci_run, ci_attempt=args.ci_attempt)
            stage = 'install'
            try:
                run_nsis(setup, install, workdir, env, root / 'install.log')
                stage = 'installed_layout'
                report['matchedRuntimeFiles'] = installed_media_matches(install, portable)
                require_scratch_registration(uninstall_entries(winreg.HKEY_CURRENT_USER), install, version)
                assert not cumulus_entries(uninstall_entries(winreg.HKEY_LOCAL_MACHINE)), (
                    'installer wrote machine-wide uninstall registration')
                report['actualNsisInstall'] = True
                report['scratchRegistrationVerified'] = True
                stage = 'ffprobe'
                verify_ffprobe(install, workdir, env)
                report['installedFfprobe'] = True
                stage = 'graphics'
                subprocess.run([sys.executable, '-E', '-X', 'utf8', str(graphics), '--binary', str(install / 'cumulus.exe'),
                                '--root', str(root / 'graphics')], cwd=workdir, env=env, check=True,
                               timeout=240)
                report['installedGraphicsDiagnostic'] = True
            except Exception as error:
                report['failedStage'] = stage
                report['errorType'] = type(error).__name__
                raise
            finally:
                stage = 'uninstall'
                try:
                    # Run the generated uninstaller in place so the parent
                    # observes completion; the disposable profile is still discarded.
                    if os.path.lexists(install):
                        plain(install, stat.S_ISDIR)
                        uninstaller = install / 'uninstall.exe'
                        plain(uninstaller, stat.S_ISREG)
                        run_nsis(uninstaller, install, workdir, env, root / 'uninstall.log', remove=True)
                        assert not any(os.path.lexists(install / p.name) for p in portable.iterdir()), (
                            'uninstaller left installed runtime files')
                        for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
                            assert not cumulus_entries(uninstall_entries(hive)), (
                                'uninstaller left Cumulus registration')
                        report['uninstallVerified'] = True
                except Exception as error:
                    report['cleanupErrorType'] = type(error).__name__
                    raise
            assert report['uninstallVerified'], 'installed package was not uninstalled'
        report['ok'] = True
    except Exception as error:
        report.setdefault('failedStage', stage)
        report.setdefault('errorType', type(error).__name__)
        raise
    finally:
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
