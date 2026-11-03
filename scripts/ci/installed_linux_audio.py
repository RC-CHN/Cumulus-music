"""Optional local-audio proof for the unmodified Debian executable."""
from contextlib import ExitStack, closing, contextmanager
import ctypes as c
import hashlib
import json
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import tempfile
import time


TITLE = 'Synthetic Playback Check'


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def wait_for(label, check, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(.15)
    raise AssertionError(f'timed out waiting for {label}')


@contextmanager
def isolated_audio(root, env):
    for name in ('openbox', 'ffmpeg', 'pipewire', 'pipewire-pulse', 'wireplumber', 'pactl', 'pw-dump'):
        assert shutil.which(name), f'local playback proof requires {name}'
    assert not Path('/dev/snd').exists(), 'hide host audio devices (bwrap --dev /dev)'
    for name in tuple(env):
        if name.startswith(('PIPEWIRE_', 'PULSE_', 'ALSA_', 'WIREPLUMBER_')):
            del env[name]
    env['GTK_USE_PORTAL'] = '0'
    with ExitStack() as stack:
        private = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix='cumulus-deb-audio-', dir='/tmp')))
        runtime = private/'runtime'
        runtime.mkdir(mode=0o700)
        env['DBUS_SYSTEM_BUS_ADDRESS'] = f'unix:path={runtime}/no-system-bus'
        env['PIPEWIRE_RUNTIME_DIR'] = str(runtime)
        env['PULSE_SERVER'] = f'unix:{runtime}/pulse/native'
        server_env = {**env, 'XDG_RUNTIME_DIR': str(runtime)}
        alsa = private/'alsa.conf'
        alsa.write_text('pcm.!default { type null }\n', encoding='ascii')
        env['ALSA_CONFIG_PATH'] = str(alsa)

        def spawn(name, *args, process_env=server_env):
            log = stack.enter_context((root/f'{name}.log').open('xb'))
            process = subprocess.Popen([name, *args], env=process_env, stdout=log, stderr=log)
            stack.callback(stop, process)
            return process

        core = spawn('pipewire')
        wait_for('private PipeWire socket', lambda: core.poll() is None and (runtime/'pipewire-0').is_socket())
        pulse = spawn('pipewire-pulse')
        policy = spawn('wireplumber')

        def pulse_ready():
            assert pulse.poll() is None and policy.poll() is None, 'private audio service exited'
            return subprocess.run(['pactl', 'info'], env=server_env, capture_output=True, timeout=3).returncode == 0

        wait_for('private Pulse socket', pulse_ready)
        subprocess.run(['pactl', 'load-module', 'module-null-sink', 'sink_name=cumulus_private'],
                       env=server_env, check=True, capture_output=True, timeout=5)
        subprocess.run(['pactl', 'set-default-sink', 'cumulus_private'],
                       env=server_env, check=True, timeout=5)
        wm = spawn('openbox', '--sm-disable', process_env=env)

        def wm_ready():
            assert wm.poll() is None, 'private window manager exited'
            return 'window id #' in subprocess.check_output(['xprop', '-root', '_NET_SUPPORTING_WM_CHECK'], text=True, timeout=3)

        wait_for('private window manager', wm_ready)
        media = private/'music'
        media.mkdir()
        source = media/'Synthetic.flac'
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i',
                        'sine=frequency=440:sample_rate=48000:duration=30',
                        '-metadata', f'title={TITLE}', source], check=True, timeout=20)
        yield source


class Input:
    def __init__(self, x, y):
        self.x, self.y = x, y
        self.x11, self.xtst = c.CDLL('libX11.so.6'), c.CDLL('libXtst.so.6')
        self.x11.XOpenDisplay.argtypes = [c.c_char_p]
        self.x11.XOpenDisplay.restype = c.c_void_p
        self.x11.XFlush.argtypes = self.x11.XCloseDisplay.argtypes = [c.c_void_p]
        self.x11.XStringToKeysym.argtypes = [c.c_char_p]
        self.x11.XStringToKeysym.restype = c.c_ulong
        self.x11.XKeysymToKeycode.argtypes = [c.c_void_p, c.c_ulong]
        self.x11.XKeysymToKeycode.restype = c.c_ubyte
        self.xtst.XTestFakeMotionEvent.argtypes = [c.c_void_p, c.c_int, c.c_int, c.c_int, c.c_ulong]
        self.xtst.XTestFakeButtonEvent.argtypes = [c.c_void_p, c.c_uint, c.c_int, c.c_ulong]
        self.xtst.XTestFakeKeyEvent.argtypes = [c.c_void_p, c.c_uint, c.c_int, c.c_ulong]
        self.display = self.x11.XOpenDisplay(None)
        assert self.display, 'private X11 input display missing'

    def close(self):
        self.x11.XCloseDisplay(self.display)

    def click(self, x, y):
        assert self.xtst.XTestFakeMotionEvent(self.display, -1, self.x+x, self.y+y, 0)
        for down in (1, 0):
            assert self.xtst.XTestFakeButtonEvent(self.display, 1, down, 0)
        self.x11.XFlush(self.display)

    def key(self, key, modifier=None):
        def send(name, down):
            code = self.x11.XKeysymToKeycode(self.display, self.x11.XStringToKeysym(name.encode('ascii')))
            assert code and self.xtst.XTestFakeKeyEvent(self.display, code, down, 0), name

        if modifier:
            send(modifier, 1)
        send(key, 1)
        send(key, 0)
        if modifier:
            send(modifier, 0)
        self.x11.XFlush(self.display)


def read_playback(database):
    with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True, timeout=.5)) as db:
        rows = db.execute(
            "SELECT key,value FROM state WHERE owner='local-guest' AND kind='playback' "
            "AND key IN ('queue','position')"
        ).fetchall()
    return {key: json.loads(value) for key, value in rows}


def private_pipewire_stream(pid, env):
    objects = json.loads(subprocess.check_output(['pw-dump'], env=env, timeout=4))
    props = {item['id']: item.get('info', {}).get('props', {}) for item in objects}
    own_clients = {str(key) for key, row in props.items() if str(row.get('application.process.id')) == str(pid)}
    sink = {item for item, row in props.items() if row.get('node.name') == 'cumulus_private'}
    streams = {item for item, row in props.items()
               if row.get('media.class') == 'Stream/Output/Audio' and
               (str(row.get('client.id')) in own_clients or str(row.get('application.process.id')) == str(pid))}
    return any(item.get('info', {}).get('output-node-id') in streams and
               item.get('info', {}).get('input-node-id') in sink for item in objects)


def verify_playback(root, app, window, x, y, width, height, source, env, screenshot):
    assert (width, height) == (1120, 760), 'local playback coordinates require the ordinary 1120x760 X11 layout'
    database = root/'state/core.sqlite3'

    def active_window():
        value = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True, timeout=3)
        match = re.search(r'window id # (0x[0-9a-f]+)', value)
        if not match:
            return None
        active = int(match[1], 16)
        owner = subprocess.run(['xprop', '-id', hex(active), '_NET_WM_PID'], capture_output=True, text=True, timeout=3)
        return active if owner.returncode == 0 and owner.stdout.strip().endswith(f'= {app.pid}') else None

    def indexed():
        with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True, timeout=.5)) as db:
            rows = db.execute("SELECT value FROM state WHERE owner='local' AND kind='library_files_v1'").fetchall()
        return next((record['id'] for record in (json.loads(value) for value, in rows)
                     if record['path'] == str(source) and record['metadata']['title'] == TITLE
                     and not record['missing'] and not record['unreadable']), None)

    def position(state, after):
        assert app.poll() is None, 'installed app exited during playback'
        rows = read_playback(database)
        queue, position = rows.get('queue'), rows.get('position')
        if not queue or not position or position['state'] != state or position['seconds'] <= after:
            return None
        item, = queue['items']
        assert item['source']['kind'] == 'local' and item['resource']['local_id'] == local_id
        assert item['resource']['metadata']['title'] == TITLE
        assert item['resource']['local_revision']['fingerprint']['bytes'] == source.stat().st_size
        assert queue['current'] == item['entry_id'] == position['entry_id']
        return position

    with closing(Input(x, y)) as inputs:
        wait_for('installed main window focus', lambda: active_window() == window)
        inputs.click(62, 520)  # Local Music
        time.sleep(2)
        screenshot('installed-local-page.png')
        inputs.click(661, 448)  # Choose directory
        wait_for('native file picker focus', lambda: active_window() not in (None, window))
        inputs.key('l', 'Control_L')
        time.sleep(.35)
        for char in str(source.parent):
            inputs.key({'/': 'slash', '-': 'minus', '_': 'underscore'}.get(char, char))
        inputs.key('Return')
        wait_for('installed main window focus after selection', lambda: active_window() == window)
        local_id = wait_for('generated FLAC indexed by the installed app', indexed)
        time.sleep(1.7)  # Allow one WebView local-index refresh after the native commit.
        screenshot('installed-local-imported.png')
        inputs.click(316, 174)  # Play all in the local list.
        first = wait_for('native playing position', lambda: position('playing', .15))
        screenshot('installed-local-playing.png')
        next_position = wait_for('native position advancement', lambda: position('playing', first['seconds']+1))
        assert next_position['entry_id'] == first['entry_id']
        wait_for('installed app linked to private PipeWire sink', lambda: private_pipewire_stream(app.pid, env), 6)
        inputs.click(559, 712)  # Pause.
        paused = wait_for('native paused position', lambda: position('paused', first['seconds']))
        assert paused['entry_id'] == first['entry_id'] and paused['paused']
        # Let the audio backend settle its final buffered clock sample, then
        # compare paused snapshots across another native persistence interval.
        time.sleep(.35)
        settled = position('paused', first['seconds'])
        assert settled and settled['entry_id'] == paused['entry_id']
        time.sleep(1.2)
        held = position('paused', first['seconds'])
        assert held and held['entry_id'] == settled['entry_id']
        assert abs(held['seconds'] - settled['seconds']) < .05, 'clock changed after pause'
        screenshot('installed-local-paused.png')
    return {'nativePositionAdvanced': True, 'pauseHeld': True, 'syntheticLocalSource': True,
            'privatePipeWireNullSink': True, 'physicalAudioOutputVerified': False,
            'firstSeconds': first['seconds'], 'laterSeconds': next_position['seconds'],
            'pausedSeconds': held['seconds'],
            'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest()}
