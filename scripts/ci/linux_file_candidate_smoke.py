"""Exercise the exact Debian desktop entry through GIO in disposable XDG roots."""
import argparse
from configparser import ConfigParser
from contextlib import ExitStack, closing
import ctypes
import json
import os
from pathlib import Path
import re
import signal
import socket
import sqlite3
import subprocess
import time

from diagnostics import verify_proof_databases
from installed_linux_audio import (
    Input, isolated_audio, private_pipewire_stream, read_playback, sink_tone, start_accessibility,
    stop, wait_for,
)


DESKTOP_ID = 'Cumulus.desktop'
OTHER_ID = 'org.example.FixturePlayer.desktop'
TYPES = ('audio/mpeg', 'audio/flac')


class Child:
    """A GIO-managed child launched with DO_NOT_REAP_CHILD."""
    def __init__(self, pid):
        self.pid = pid
        self.result = None

    def poll(self):
        if self.result is None:
            waited, status = os.waitpid(self.pid, os.WNOHANG)
            if waited:
                self.result = os.waitstatus_to_exitcode(status)
        return self.result

    def wait(self, timeout):
        deadline = time.monotonic() + timeout
        while self.poll() is None:
            if time.monotonic() >= deadline:
                raise subprocess.TimeoutExpired('packaged Cumulus', timeout)
            time.sleep(.05)
        return self.result

    def terminate(self):
        if self.poll() is None:
            os.kill(self.pid, signal.SIGTERM)

    def kill(self):
        if self.poll() is None:
            os.kill(self.pid, signal.SIGKILL)


def isolated_roots(root, package):
    xdg = {}
    for key, suffix in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                        ('XDG_CONFIG_DIRS', 'system-config'), ('XDG_CACHE_HOME', 'cache')]:
        folder = root / suffix
        folder.mkdir(mode=0o700)
        xdg[key] = str(folder)
    other_apps = Path(xdg['XDG_DATA_HOME']) / 'applications'
    other_apps.mkdir()
    (other_apps / OTHER_ID).write_text(
        '[Desktop Entry]\nType=Application\nName=Synthetic previous default\n'
        'Exec=/usr/bin/false %F\nMimeType=audio/mpeg;audio/flac;\n', encoding='utf-8')
    config = Path(xdg['XDG_CONFIG_HOME'])
    (config / 'mimeapps.list').write_text('[Default Applications]\n' + ''.join(
        f'{mime_type}={OTHER_ID};\n' for mime_type in TYPES), encoding='utf-8')
    baseline = defaults_state(root)
    assert len(baseline) == 1
    subprocess.run(['update-desktop-database', str(other_apps)], check=True, timeout=10)
    uninstalled = root / 'uninstalled-system-data'
    uninstalled.mkdir()
    (uninstalled / 'mime').symlink_to('/usr/share/mime', target_is_directory=True)
    clean = dict(os.environ, **xdg, XDG_DATA_DIRS=str(uninstalled), LC_ALL='C')
    for mime_type in TYPES:
        output = subprocess.check_output(['gio', 'mime', mime_type], env=clean, text=True, timeout=5)
        assert f'Default application for “{mime_type}”: {OTHER_ID}' in output or (
            f'Default application for "{mime_type}": {OTHER_ID}' in output)
        assert DESKTOP_ID not in output

    extracted = root / 'extracted'
    subprocess.run(['dpkg-deb', '-x', str(package), str(extracted)], check=True, timeout=60)
    applications = extracted / 'usr/share/applications'
    entry = applications / DESKTOP_ID
    binary = (extracted / 'usr/bin/cumulus').resolve(strict=True)
    assert binary.is_file() and entry.is_file() and not entry.is_symlink()
    assert {path.name for path in applications.glob('*.desktop')} == {DESKTOP_ID}
    subprocess.run(['desktop-file-validate', str(entry)], check=True, timeout=4)
    parser = ConfigParser(interpolation=None)
    parser.read(entry, encoding='utf-8')
    fields = parser['Desktop Entry']
    assert fields['Exec'] == 'cumulus --open-local-files %F'
    assert fields['StartupWMClass'] == fields['Icon'] == 'cumulus'
    assert fields['Name'] == 'Cumulus' and fields['Type'] == 'Application'
    assert fields['Terminal'] == 'false' and 'Music' in fields['Categories'].split(';')
    assert fields['MimeType'].split(';') == [*TYPES, '']
    assert fields.get('DBusActivatable', 'false').lower() == 'false'
    controls = root / 'package-control'
    subprocess.run(['dpkg-deb', '-e', str(package), str(controls)], check=True, timeout=15)
    for name in ('preinst', 'postinst', 'prerm', 'postrm'):
        if (controls / name).is_file():
            script = (controls / name).read_text(encoding='utf-8')
            assert all(word not in script for word in ('mimeapps.list', 'xdg-mime', 'gio mime'))

    # GIO reads the extracted application menu and this system MIME database
    # only. Its candidate cache is generated inside the extracted scratch tree.
    mime = extracted / 'usr/share/mime'
    assert not mime.exists()
    mime.symlink_to('/usr/share/mime', target_is_directory=True)
    subprocess.run(['update-desktop-database', str(applications)], check=True, timeout=10)
    xdg['XDG_DATA_DIRS'] = str(extracted / 'usr/share')
    assert defaults_state(root) == baseline
    return binary, entry, xdg, baseline


def defaults_state(root):
    return {path.relative_to(root): path.read_bytes()
            for path in root.rglob('*mimeapps.list') if path.is_file()}


def windows():
    raw = subprocess.check_output(['xprop', '-root', '_NET_CLIENT_LIST'], text=True, timeout=3)
    observed = {}
    for window in re.findall(r'0x[0-9a-f]+', raw):
        result = subprocess.run(['xprop', '-id', window, '_NET_WM_PID'], text=True,
                                capture_output=True, timeout=3)
        if not result.returncode and (match := re.search(r'= (\d+)\s*$', result.stdout)):
            observed[window] = int(match[1])
    return observed


def focused(child):
    assert child.poll() is None, f'packaged app exited with {child.result}'
    raw = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True, timeout=3)
    match = re.search(r'window id # (0x[0-9a-f]+)', raw)
    return match[1] if match and windows().get(match[1]) == child.pid else None


def playing(database, child, path, elapsed=.6):
    assert child.poll() is None, f'packaged player exited with {child.result}'
    if not database.is_file():
        return None
    state = read_playback(database)
    queue, clock = state.get('queue'), state.get('position')
    if not queue or not clock or clock['state'] != 'playing' or clock['seconds'] < elapsed:
        return None
    selected = next((item for item in queue['items'] if item['entry_id'] == queue['current']), None)
    if (selected and clock['entry_id'] == selected['entry_id']
            and selected['resource']['native_open_file']['path'] == str(path)):
        return state
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--deb', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--outer-network', required=True)
    args = parser.parse_args()
    namespace = os.readlink('/proc/self/ns/net')
    assert re.fullmatch(r'net:\[[0-9]+\]', args.outer_network) and namespace != args.outer_network
    assert {name for _, name in socket.if_nameindex()} == {'lo'}, 'requires isolated loopback only'
    assert not Path('/dev/snd').exists(), 'hide real host audio devices'
    assert re.fullmatch(r':[0-9]+', os.environ['DISPLAY']) and os.environ['GDK_BACKEND'] == 'x11'
    assert Path(os.environ['XAUTHORITY']).is_file()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    binary, entry, xdg, baseline = isolated_roots(root, args.deb.resolve(strict=True))
    # Set discovery roots before importing GIO, which caches some XDG paths.
    for key in tuple(os.environ):
        if key.startswith('CUMULUS_'):
            del os.environ[key]
    os.environ.update(xdg)
    os.environ['PATH'] = os.pathsep.join((str(binary.parent), os.environ['PATH']))
    env = os.environ.copy()
    env['CUMULUS_DEVICE_IDENTITY'] = 'native'
    import gi
    gi.require_version('Gio', '2.0')
    from gi.repository import Gio, GLib
    try:
        gi.require_version('GioUnix', '2.0')
    except ValueError:
        desktop_info = Gio.DesktopAppInfo
    else:
        from gi.repository import GioUnix
        desktop_info = GioUnix.DesktopAppInfo
    start_accessibility()
    app = desktop_info.new(DESKTOP_ID)
    assert app and Path(app.get_filename()) == entry and app.supports_files() and not app.supports_uris()

    def verify_candidates():
        for mime_type in TYPES:
            default = Gio.AppInfo.get_default_for_type(mime_type, False)
            assert default and default.get_id() == OTHER_ID, f'explicit default changed for {mime_type}'
            assert DESKTOP_ID in {candidate.get_id() for candidate in Gio.AppInfo.get_recommended_for_type(mime_type)}
        for mime_type in ('audio/x-wav', 'video/mp4', 'x-scheme-handler/http', 'x-scheme-handler/https'):
            assert DESKTOP_ID not in {candidate.get_id() for candidate in Gio.AppInfo.get_all_for_type(mime_type)}
        assert defaults_state(root) == baseline, 'desktop launch changed an XDG defaults file'

    database = Path(xdg['XDG_DATA_HOME']) / 'io.cumulus.music/core.sqlite3'
    verify_candidates()
    with isolated_audio(root, env) as seed, ExitStack() as children:
        def launch(name, *uris):
            context = Gio.AppLaunchContext()
            for key, value in env.items():
                context.setenv(key, value)
            for key in os.environ.keys() - env.keys():
                context.unsetenv(key)
            launched = []

            def track(_, pid, _data):
                child = Child(pid)
                children.callback(stop, child)
                launched.append(child)

            with (root / f'{name}.log').open('xb') as output:
                assert app.launch_uris_as_manager_with_fds(list(uris), context, GLib.SpawnFlags.DO_NOT_REAP_CHILD,
                                                          None, None, track, None, -1, output.fileno(), output.fileno())
            assert len(launched) == 1, 'unexpected number of GIO launch processes'
            return launched[0]

        def check_owner(child):
            window = wait_for('GIO-launched packaged main window focused', lambda: focused(child))
            assert Path(f'/proc/{child.pid}/exe').resolve(strict=True) == binary
            assert os.readlink(f'/proc/{child.pid}/ns/net') == namespace
            return window

        first = root / '1 合成 % "原曲".flac'
        second = root / '2 合成 % "转交".mp3'
        unsupported = root / 'unsupported.wav'
        first.write_bytes(seed.read_bytes())
        for path, frequency, length in [(second, 10000, 45), (unsupported, 60, 5)]:
            subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i',
                            f'sine=frequency={frequency}:sample_rate=48000:duration={length}', str(path)],
                           check=True, timeout=15)
        for path, kind in [(first, 'audio/flac'), (second, 'audio/mpeg')]:
            observed = Gio.File.new_for_path(str(path)).query_info('standard::content-type', Gio.FileQueryInfoFlags.NONE, None)
            assert observed.get_content_type() == kind
        cold = launch('cold', first.as_uri())
        check_owner(cold)
        before = wait_for('cold GIO path actually decoded', lambda: playing(database, cold, first))
        wait_for('cold app linked to private monitor', lambda: private_pipewire_stream(cold.pid, env))
        initial = sink_tone(env)
        assert initial['amplitude440Hz'] > 200
        stop(cold)
        # Establish an isolated previous queue and set only the synthetic
        # startup preference; an empty %F launch must keep ordinary autoplay.
        with closing(sqlite3.connect(database)) as db:
            values = [('current', {'revision': 1, 'settings': {'autoPlay': True, 'rememberPosition': True}}), ('revision', 1)]
            for key, value in values:
                db.execute("INSERT OR REPLACE INTO state(owner,kind,key,value) VALUES (?,?,?,?)",
                           ('local-device', 'playback-startup-preferences-v1', key, json.dumps(value)))
            db.commit()
        prior_seconds = read_playback(database)['position']['seconds']
        owner = launch('ordinary')
        window = check_owner(owner)
        wait_for('ordinary no-file launcher owns a new native output stream',
                 lambda: private_pipewire_stream(owner.pid, env))
        restored = wait_for('ordinary no-file launcher restored and advanced saved audio',
                            lambda: playing(database, owner, first, prior_seconds + .6))
        assert restored['queue'] == before['queue'] and owner.pid != cold.pid
        duplicate = launch('ordinary-duplicate')
        assert (result := duplicate.wait(20)) == 2, result
        denied = launch('unsupported', unsupported.as_uri())
        assert (result := denied.wait(20)) == 3, result
        assert read_playback(database)['queue'] == restored['queue']

        with closing(Input(0, 0)) as inputs:
            iconify = inputs.x11.XIconifyWindow
            iconify.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int]
            iconify.restype = ctypes.c_int
            assert iconify(inputs.display, int(window, 16), 0)
            inputs.x11.XFlush(inputs.display)
        wait_for('original GIO window was minimized', lambda: focused(owner) != window)
        sender = launch('warm', second.as_uri(), first.as_uri())
        assert sender.wait(20) == 0 and sender.pid != owner.pid
        after = wait_for('warm GIO multi-file launch reached original player', lambda: playing(database, owner, second))
        wait_for('warm GIO launch restored same window', lambda: focused(owner) == window)
        assert windows() == {window: owner.pid}
        assert [row['resource']['native_open_file']['path'] for row in after['queue']['items']] == [str(second), str(first)]
        wait_for('same owner output stayed on private sink', lambda: private_pipewire_stream(owner.pid, env))
        final = sink_tone(env)
        assert final['amplitude10000Hz'] > 200 and final['amplitude440Hz'] < initial['amplitude440Hz'] * .1
        verify_candidates()

    verify_proof_databases([root])
    result = dict(ok=True, extractedDebianDesktopEntry=True, realGioFileActivation=True,
                  syntheticDatabaseIdentityAuditPassed=True,
                  onlyMp3AndFlacCandidates=True, existingDefaultsPreservedByteForByte=True,
                  ordinaryNoFileLauncherRestoredAutoplay=True, ordinaryDuplicateDidNotTransfer=True,
                  coldNativeUnicodeFile=True,
                  warmGioMultiFileSameProcessAndWindow=True, unsupportedWavKeptPreviousQueue=True,
                  noUriHandler=True, coldPcm=initial, warmPcm=final, physicalOutputVerified=False,
                  successfulMusicServiceRequests=0)
    (root / 'report.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
