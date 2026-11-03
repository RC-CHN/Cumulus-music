"""Start the exact extracted Debian app, with no account or external network.

Optional playback needs a private 1280x900 Xvfb display, D-Bus session and a
network namespace with no routes. Hide host sound devices, for example:
  xvfb-run -a -s '-screen 0 1280x900x24' bwrap --unshare-user --unshare-net \
    --die-with-parent --bind / / --dev /dev --proc /proc -- dbus-run-session -- \
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
import subprocess
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


def observe(binary, root, verify_local_playback=False):
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
    if verify_local_playback:
        from installed_linux_audio import isolated_audio, verify_playback
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
                                        playback = verify_playback(root, app, window, x, y, width, height, source, env, screenshot)
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
    parser.add_argument('--packages', type=Path, required=True)
    parser.add_argument('--filter', type=Path, required=True)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--verify-local-playback', action='store_true',
                        help='exercise synthetic audio via X11 and a private PipeWire null sink')
    args = parser.parse_args()
    require_isolation()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    manifest = json.loads((args.packages/'manifest-linux-x64.json').read_text(encoding='utf-8'))
    assert manifest['commit'] == args.commit and manifest['platform'] == 'linux-x64'
    name = f"Cumulus-{manifest['version']}-linux-x64.deb"
    asset, = (row for row in manifest['assets'] if row['name'] == name)
    deb = args.packages/name
    assert deb.stat().st_size == asset['bytes'] and digest(deb) == asset['sha256']
    with tempfile.TemporaryDirectory(prefix='cumulus-deb-proof-') as extracted:
        subprocess.run(['dpkg-deb', '--extract', deb, extracted], check=True)
        installed = Path(extracted)/'usr'
        executable = installed/'bin/cumulus'
        packaged_filter = installed/'lib/Cumulus/native/libcumulus_audio_filter.so'
        assert executable.is_file() and not executable.is_symlink() and os.access(executable, os.X_OK)
        assert packaged_filter.is_file() and not packaged_filter.is_symlink()
        assert digest(packaged_filter) == digest(args.filter)
        width, height, playback = observe(executable, root, args.verify_local_playback)
    result = dict(ok=True, commit=args.commit, archiveVerified=True,
                  installedFilterMatchesPortable=True, ordinaryWindowPainted=True,
                  noExternalNetwork=True, width=width, height=height)
    if playback is not None:
        result['localPlayback'] = playback
    (root/'report.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
