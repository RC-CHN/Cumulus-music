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

from debian_version import verify_debian_version
from linux_flac_artifacts import (
    DEBIAN_NATIVE, LIBRARY, MAX_NOTICE, NOTICE, validate_receipt, verify_extracted,
)
from installed_linux_audio import INDEXED_FLAC_BLOCK_SIZES
from release_guard import VERSION


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def require_isolation():
    links = json.loads(subprocess.check_output(['ip', '-j', 'link']))
    assert [row['ifname'] for row in links] == ['lo'], 'probe requires its own network namespace'
    for family in ('-4', '-6'):
        routes = subprocess.check_output(['ip', family, '-j', 'route', 'show', 'table', 'main'])
        assert json.loads(routes) == [], 'probe requires no external routes'


def unpack_portable(archive, directory, prefix, *, flac_decoder=None):
    """Copy only the known app files from the verified tar, without archive paths."""
    filenames = ['cumulus', 'libcumulus_audio_filter.so']
    if flac_decoder is not None:
        library = validate_receipt(flac_decoder)
        filenames.extend((LIBRARY, NOTICE))
    with tarfile.open(archive, 'r:gz') as package:
        members = package.getmembers()
        for filename in filenames:
            member, = (row for row in members if row.name == f'{prefix}/{filename}')
            assert member.isfile() and member.size > 0
            if filename == 'cumulus':
                assert member.mode & 0o111, 'portable executable is not marked executable'
            if filename == LIBRARY:
                assert member.size == library['bytes'], 'portable FLAC library size mismatch'
            elif filename == NOTICE:
                assert member.size <= MAX_NOTICE, 'portable FLAC notice exceeds bounds'
            destination = directory / filename
            destination.parent.mkdir(parents=True, exist_ok=True)
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


def observe(binary, root, verify_local_playback=False, packaged_filter=None,
            verify_monitor_pcm=False, *, software_rendering=False, indexed_flac_library=None,
            indexed_flac_block_size=None, verify_indexed_playback_speeds=False):
    import gi
    gi.require_version('Gdk', '3.0')
    gi.require_version('GdkX11', '3.0')
    from gi.repository import Gdk, GdkX11

    env = {key: value for key, value in os.environ.items()
           if not key.startswith('CUMULUS_') and key != 'APPDIR' and 'proxy' not in key.lower()}
    env['CUMULUS_DEVICE_IDENTITY'] = 'native'
    env['CUMULUS_DATA_DIR'] = str(root/'state')
    if software_rendering:
        env['CUMULUS_RENDERER'] = 'software'
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
        accessibility = start_accessibility()
        audio = isolated_audio(root, env, indexed_flac_block_size=indexed_flac_block_size)
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
                                            packaged_filter, verify_monitor_pcm, accessibility,
                                            indexed_flac_library=indexed_flac_library,
                                            indexed_flac_block_size=indexed_flac_block_size,
                                            verify_indexed_playback_speeds=verify_indexed_playback_speeds)
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
    parser.add_argument('--software-rendering', action='store_true',
                        help='exercise the pinned native software profile through ordinary startup')
    parser.add_argument('--verify-local-playback', action='store_true',
                        help='exercise synthetic audio via X11 and a private PipeWire null sink')
    parser.add_argument('--verify-installed-equalizer', action='store_true',
                        help='also exercise an EQ preset and the package filter while playing')
    parser.add_argument('--verify-packaged-flac', action='store_true',
                        help='load the audited FLAC bytes from the exact extracted package in an identity probe')
    parser.add_argument('--verify-monitor-pcm', action='store_true',
                        help='measure the synthetic tone and pause silence at the private sink')
    parser.add_argument('--verify-indexed-flac-equalizer', action='store_true',
                        help='reselect an indexed stereo FLAC and measure the trusted native graph')
    parser.add_argument('--indexed-flac-block-size', type=int, choices=INDEXED_FLAC_BLOCK_SIZES,
                        help='indexed fixture STREAMINFO block size (default: 1024)')
    parser.add_argument('--verify-indexed-playback-speeds', action='store_true',
                        help='exercise live 2×, 4× and restored 1× on the same indexed graph')
    parser.add_argument('--indexed-flac-sha256',
                        help='compile-time pinned FLAC digest for an ordinary developer binary')
    args = parser.parse_args()
    if args.verify_monitor_pcm and not args.verify_local_playback:
        parser.error('--verify-monitor-pcm needs --verify-local-playback')
    if args.portable and not args.packages:
        parser.error('--portable needs --packages')
    if args.verify_packaged_flac and not args.packages:
        parser.error('--verify-packaged-flac needs --packages')
    if (args.ci_run or args.ci_attempt) and not (
        args.packages and all(re.fullmatch(r'[1-9][0-9]*', value or '')
                              for value in (args.ci_run, args.ci_attempt))
    ):
        parser.error('--ci-run and --ci-attempt need positive IDs and --packages')
    if bool(args.packages) != bool(args.filter):
        parser.error('--filter is required with --packages and must be omitted with --binary')
    if args.verify_installed_equalizer and not (args.packages and args.verify_local_playback):
        parser.error('--verify-installed-equalizer needs --packages and --verify-local-playback')
    indexed_attestation = args.verify_packaged_flac if args.packages else args.indexed_flac_sha256
    if args.verify_indexed_flac_equalizer and not (
        args.verify_local_playback and args.verify_monitor_pcm and not args.verify_installed_equalizer
        and indexed_attestation
    ):
        parser.error('--verify-indexed-flac-equalizer needs --verify-local-playback and '
                     '--verify-monitor-pcm, plus --verify-packaged-flac with --packages or '
                     '--indexed-flac-sha256 with --binary; run it separately from installed LADSPA')
    if args.indexed_flac_sha256 and not (args.binary and args.verify_indexed_flac_equalizer
                                         and re.fullmatch(r'[0-9a-f]{64}', args.indexed_flac_sha256)):
        parser.error('--indexed-flac-sha256 needs --binary and --verify-indexed-flac-equalizer')
    if args.indexed_flac_block_size is not None and not args.verify_indexed_flac_equalizer:
        parser.error('--indexed-flac-block-size needs --verify-indexed-flac-equalizer')
    if args.verify_indexed_playback_speeds and not args.verify_indexed_flac_equalizer:
        parser.error('--verify-indexed-playback-speeds needs --verify-indexed-flac-equalizer')
    indexed_flac_block_size = (
        (args.indexed_flac_block_size or 1024) if args.verify_indexed_flac_equalizer else None)
    require_isolation()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    if args.binary:
        binary = args.binary.resolve(strict=True)
        assert binary.is_file() and os.access(binary, os.X_OK), 'expected a built executable'
        binary_hash = digest(binary)
        flac = None
        if args.verify_indexed_flac_equalizer:
            flac = binary.with_name(LIBRARY)
            assert (not flac.is_symlink() and flac.is_file()
                    and digest(flac) == args.indexed_flac_sha256), 'wrong pinned developer FLAC sibling'
        width, height, playback = observe(binary, root, args.verify_local_playback,
                                         verify_monitor_pcm=args.verify_monitor_pcm,
                                         software_rendering=args.software_rendering,
                                         indexed_flac_library=flac,
                                         indexed_flac_block_size=indexed_flac_block_size,
                                         verify_indexed_playback_speeds=args.verify_indexed_playback_speeds)
        assert digest(binary) == binary_hash, 'executable changed during the probe'
        result = dict(ok=True, commit=args.commit, ordinaryBinary=True, binarySha256=binary_hash)
        if flac is not None:
            assert digest(flac) == args.indexed_flac_sha256, 'pinned developer FLAC sibling changed'
            result['developerFlacSha256'] = args.indexed_flac_sha256
    else:
        manifest = json.loads((args.packages/'manifest-linux-x64.json').read_text(encoding='utf-8'))
        assert manifest['commit'] == args.commit and manifest['platform'] == 'linux-x64'
        if args.ci_run:
            assert (manifest['ciRun'], manifest['ciAttempt']) == (args.ci_run, args.ci_attempt), (
                'wrong package CI run or attempt')
        flac_decoder = manifest.get('flacDecoder') if args.verify_packaged_flac else None
        if args.verify_packaged_flac:
            validate_receipt(flac_decoder)
        version = manifest['version']
        assert VERSION.fullmatch(version), 'invalid package version'
        prefix = f'Cumulus-{version}-linux-x64'
        name = prefix + ('.tar.gz' if args.portable else '.deb')
        asset, = (row for row in manifest['assets'] if row['name'] == name)
        archive = args.packages/name
        assert archive.stat().st_size == asset['bytes'] and digest(archive) == asset['sha256']
        debian_version = None if args.portable else verify_debian_version(archive, version)
        with tempfile.TemporaryDirectory(prefix='cumulus-package-proof-') as extracted:
            directory = Path(extracted)
            if args.portable:
                executable, packaged_filter = unpack_portable(archive, directory, prefix,
                                                               flac_decoder=flac_decoder)
                native = directory
            else:
                subprocess.run(['dpkg-deb', '--extract', archive, extracted], check=True)
                executable = directory/'usr/bin/cumulus'
                native = directory / DEBIAN_NATIVE
                packaged_filter = native / 'libcumulus_audio_filter.so'
            assert executable.is_file() and not executable.is_symlink() and os.access(executable, os.X_OK)
            assert packaged_filter.is_file() and not packaged_filter.is_symlink()
            assert digest(packaged_filter) == digest(args.filter)
            flac = None
            if flac_decoder is not None:
                flac = verify_extracted(native, flac_decoder,
                                        staged_library=args.filter.with_name(LIBRARY), native_probe=True)
            indexed_library = native / LIBRARY if args.verify_indexed_flac_equalizer else None
            width, height, playback = observe(executable, root, args.verify_local_playback,
                                              packaged_filter if args.verify_installed_equalizer else None,
                                              args.verify_monitor_pcm,
                                              software_rendering=args.software_rendering,
                                              indexed_flac_library=indexed_library,
                                              indexed_flac_block_size=indexed_flac_block_size,
                                              verify_indexed_playback_speeds=args.verify_indexed_playback_speeds)
            if indexed_library is not None:
                assert digest(indexed_library) == flac['sha256'], 'packaged FLAC changed during playback'
        result = dict(ok=True, commit=args.commit, archiveVerified=True, package=name,
                      packageSha256=asset['sha256'], ciRun=manifest['ciRun'],
                      ciAttempt=manifest['ciAttempt'], installedFilterMatchesPortable=True)
        if debian_version is not None:
            result['debianVersion'] = debian_version
        if flac is not None:
            result['packagedFlac'] = flac
    result.update(ordinaryWindowPainted=True, noExternalNetwork=True, width=width, height=height)
    if args.software_rendering:
        result['softwareRenderingRequested'] = True
    if playback is not None:
        result['localPlayback'] = playback
    (root/'report.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
