"""Start a built executable or exact extracted Linux package with no external network.

Optional playback needs a private 1280x900 Xvfb display, D-Bus session and a
network namespace with no routes. Hide host sound devices, for example:
  xvfb-run -a -s '-screen 0 1280x900x24' bwrap --unshare-user --unshare-net \
    --die-with-parent --bind / / --dev /dev --proc /proc -- \
    env -u AT_SPI_BUS_ADDRESS -u AT_SPI_DISPLAY -u NO_AT_BRIDGE dbus-run-session -- \
    /usr/bin/python3 scripts/ci/installed_linux_smoke.py [package arguments] \
    --verify-local-playback
"""
import argparse
from contextlib import nullcontext
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import time


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require_isolation():
    links = json.loads(subprocess.check_output(['ip', '-j', 'link']))
    assert [row['ifname'] for row in links] == ['lo'], 'probe requires its own network namespace'
    for family in ('-4', '-6'):
        routes = subprocess.check_output(['ip', family, '-j', 'route', 'show', 'table', 'main'])
        assert json.loads(routes) == [], 'probe requires no external routes'


def unpack_portable(archive, directory, prefix):
    """Copy only the executable and its adjacent filter from the verified tar."""
    with tarfile.open(archive, 'r:gz') as package:
        members = package.getmembers()
        for filename in ('cumulus', 'libcumulus_audio_filter.so'):
            member, = (row for row in members if row.name == f'{prefix}/{filename}')
            assert member.isfile() and member.size > 0
            if filename == 'cumulus':
                assert member.mode & 0o111, 'portable executable is not marked executable'
            destination = directory / filename
            with package.extractfile(member) as stream, destination.open('xb') as output:
                shutil.copyfileobj(stream, output)
            if filename == 'cumulus':
                destination.chmod(0o700)
    return directory / 'cumulus', directory / 'libcumulus_audio_filter.so'


def own_window(pid):
    tree = subprocess.run(['xwininfo', '-root', '-tree'], capture_output=True, text=True)
    if tree.returncode:
        return None
    for candidate in re.findall(r'(0x[0-9a-f]+) "Cumulus":', tree.stdout):
        owner = subprocess.run(['xprop', '-id', candidate, '_NET_WM_PID'], capture_output=True, text=True)
        if owner.returncode or not owner.stdout.strip().endswith(f'= {pid}'):
            continue
        attrs = subprocess.run(['xwininfo', '-id', candidate], capture_output=True, text=True)
        if attrs.returncode == 0 and 'IsViewable' in attrs.stdout:
            return int(candidate, 16)
    return None


def observe(binary, root, verify_local_playback=False, packaged_filter=None, verify_monitor_pcm=False):
    import gi
    gi.require_version('Gdk', '3.0')
    gi.require_version('GdkX11', '3.0')
    from gi.repository import Gdk, GdkX11

    env = {key: value for key, value in os.environ.items()
           if not key.startswith('CUMULUS_') and key != 'APPDIR' and 'proxy' not in key.lower()}
    env['CUMULUS_DEVICE_IDENTITY'] = 'native'
    env['CUMULUS_DATA_DIR'] = str(root/'state')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR', 'XDG_STATE_HOME'):
        folder = root/key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    display = Gdk.Display.get_default()
    assert display is not None, 'private Xvfb display missing'
    screen = Gdk.get_default_root_window()
    accessibility = None
    if verify_local_playback:
        from installed_linux_audio import isolated_audio, start_accessibility, verify_playback
        if packaged_filter is not None:
            accessibility = start_accessibility()
        audio = isolated_audio(root, env)
    else:
        audio = nullcontext()
    with audio as source, (root/'host.log').open('xb') as log:
        app = subprocess.Popen([binary], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 60
            while time.monotonic() < deadline:
                assert app.poll() is None, 'packaged app exited before first paint'
                window = own_window(app.pid)
                if window is not None and (root/'state/core.sqlite3').is_file():
                    surface = GdkX11.X11Window.foreign_new_for_display(display, window)
                    if surface:
                        width, height = surface.get_width(), surface.get_height()
                        x, y = surface.get_origin()[-2:]
                        if width > 500 and height > 350 and x >= 0 and y >= 0:
                            image = Gdk.pixbuf_get_from_window(screen, x, y, width, height)
                            if image is not None:
                                pixels, stride, channels = image.get_pixels(), image.get_rowstride(), image.get_n_channels()
                                points = (row*stride + col*channels for row in range(3, height-3, 13)
                                          for col in range(3, width-3, 13))
                                if len({pixels[index:index+3] for index in points}) >= 30:
                                    assert image.savev(str(root/'installed-window.png'), 'png', [], [])
                                    playback = None
                                    if source is not None:
                                        def screenshot(name):
                                            capture = Gdk.pixbuf_get_from_window(screen, x, y, width, height)
                                            assert capture is not None
                                            assert capture.savev(str(root/name), 'png', [], [])
                                        playback = verify_playback(
                                            root, app, window, x, y, width, height, source, env, screenshot,
                                            packaged_filter, verify_monitor_pcm, accessibility)
                                    return width, height, playback
                time.sleep(.2)
            raise AssertionError('packaged app never painted a normal window')
        finally:
            if app.poll() is None:
                app.terminate()
                try:
                    app.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    source = parser.add_mutually_exclusive_group(required=True)
    source.add_argument('--packages', type=Path, help='directory with Linux packages and manifest')
    source.add_argument('--binary', type=Path, help='ordinary unpackaged Cumulus executable')
    parser.add_argument('--filter', type=Path, help='portable filter to compare in package mode')
    parser.add_argument('--commit', required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--ci-run', help='require this CI run in the package manifest')
    parser.add_argument('--ci-attempt', help='require this CI attempt in the package manifest')
    parser.add_argument('--portable', action='store_true', help='verify the portable tar.gz instead of the Debian package')
    parser.add_argument('--verify-local-playback', action='store_true',
                        help='exercise synthetic audio via X11 and a private PipeWire null sink')
    parser.add_argument('--verify-installed-equalizer', action='store_true',
                        help='also exercise an EQ preset and the package filter while playing')
    parser.add_argument('--verify-monitor-pcm', action='store_true',
                        help='measure the synthetic tone and pause silence at the private sink')
    args = parser.parse_args()
    if args.verify_monitor_pcm and not args.verify_local_playback:
        parser.error('--verify-monitor-pcm needs --verify-local-playback')
    if args.portable and not args.packages:
        parser.error('--portable needs --packages')
    if (args.ci_run or args.ci_attempt) and not (
        args.packages and all(re.fullmatch(r'[1-9][0-9]*', value or '')
                              for value in (args.ci_run, args.ci_attempt))
    ):
        parser.error('--ci-run and --ci-attempt need positive IDs and --packages')
    if bool(args.packages) != bool(args.filter):
        parser.error('--filter is required with --packages and must be omitted with --binary')
    if args.verify_installed_equalizer and not (args.packages and args.verify_local_playback):
        parser.error('--verify-installed-equalizer needs --packages and --verify-local-playback')
    require_isolation()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    if args.binary:
        binary = args.binary.resolve(strict=True)
        assert binary.is_file() and os.access(binary, os.X_OK), 'expected a built executable'
        binary_hash = digest(binary)
        width, height, playback = observe(binary, root, args.verify_local_playback,
                                         verify_monitor_pcm=args.verify_monitor_pcm)
        assert digest(binary) == binary_hash, 'executable changed during the probe'
        result = dict(ok=True, commit=args.commit, ordinaryBinary=True, binarySha256=binary_hash)
    else:
        manifest = json.loads((args.packages/'manifest-linux-x64.json').read_text(encoding='utf-8'))
        assert manifest['commit'] == args.commit and manifest['platform'] == 'linux-x64'
        if args.ci_run:
            assert (manifest['ciRun'], manifest['ciAttempt']) == (args.ci_run, args.ci_attempt), (
                'wrong package CI run or attempt')
        version = manifest['version']
        assert re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', version), 'invalid package version'
        prefix = f'Cumulus-{version}-linux-x64'
        name = prefix + ('.tar.gz' if args.portable else '.deb')
        asset, = (row for row in manifest['assets'] if row['name'] == name)
        archive = args.packages/name
        assert archive.stat().st_size == asset['bytes'] and digest(archive) == asset['sha256']
        with tempfile.TemporaryDirectory(prefix='cumulus-package-proof-') as extracted:
            directory = Path(extracted)
            if args.portable:
                executable, packaged_filter = unpack_portable(archive, directory, prefix)
            else:
                subprocess.run(['dpkg-deb', '--extract', archive, extracted], check=True)
                executable = directory/'usr/bin/cumulus'
                packaged_filter = directory/'usr/lib/Cumulus/native/libcumulus_audio_filter.so'
            assert executable.is_file() and not executable.is_symlink() and os.access(executable, os.X_OK)
            assert packaged_filter.is_file() and not packaged_filter.is_symlink()
            assert digest(packaged_filter) == digest(args.filter)
            width, height, playback = observe(executable, root, args.verify_local_playback,
                                              packaged_filter if args.verify_installed_equalizer else None,
                                              args.verify_monitor_pcm)
        result = dict(ok=True, commit=args.commit, archiveVerified=True, package=name,
                      packageSha256=asset['sha256'], ciRun=manifest['ciRun'],
                      ciAttempt=manifest['ciAttempt'], installedFilterMatchesPortable=True)
    result.update(ordinaryWindowPainted=True, noExternalNetwork=True, width=width, height=height)
    if playback is not None:
        result['localPlayback'] = playback
    (root/'report.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
