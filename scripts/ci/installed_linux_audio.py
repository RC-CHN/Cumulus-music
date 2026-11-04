"""Optional local-audio proof for the unmodified Debian executable."""
from contextlib import ExitStack, closing, contextmanager
import ctypes as c
import hashlib
import json
import math
import os
from pathlib import Path
import re
import select
import shutil
import sqlite3
import subprocess
import struct
import tempfile
import time


TITLE = 'Synthetic Playback Check'
MONITOR_TONES = (60, 440, 10000)


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
    busy_reads = 0
    while time.monotonic() < deadline:
        try:
            result = check()
        except sqlite3.OperationalError as error:
            # We observe an unmodified app's live database through an
            # independent read-only connection. Missing a sample while its
            # writer owns a lock does not say anything about playback.
            code = getattr(error, 'sqlite_errorcode', None)
            if code is None or code & 0xff not in (sqlite3.SQLITE_BUSY, sqlite3.SQLITE_LOCKED):
                raise
            busy_reads += 1
        else:
            if result:
                return result
        time.sleep(.15)
    contention = f' ({busy_reads} unavailable SQLite samples)' if busy_reads else ''
    raise AssertionError(f'timed out waiting for {label}{contention}')


@contextmanager
def isolated_audio(root, env):
    for name in ('openbox', 'ffmpeg', 'pipewire', 'pipewire-pulse', 'wireplumber', 'pactl', 'pw-dump'):
        assert shutil.which(name), f'local playback proof requires {name}'
    assert not Path('/dev/snd').exists(), 'hide host audio devices (bwrap --dev /dev)'
    for name in tuple(env):
        if name.startswith(('PIPEWIRE_', 'PULSE_', 'ALSA_', 'WIREPLUMBER_')):
            del env[name]
    env['GTK_USE_PORTAL'] = '0'
    home = root/'home'
    home.mkdir(mode=0o700)
    env['HOME'] = str(home)
    with ExitStack() as stack:
        private = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix='cumulus-deb-audio-', dir='/tmp')))
        runtime = private/'runtime'
        runtime.mkdir(mode=0o700)
        env['DBUS_SYSTEM_BUS_ADDRESS'] = f'unix:path={runtime}/no-system-bus'
        env['PIPEWIRE_RUNTIME_DIR'] = str(runtime)
        env['PULSE_SERVER'] = f'unix:{runtime}/pulse/native'
        # GTK's AT-SPI bridge creates a Unix socket below XDG_RUNTIME_DIR
        # in the app process too. The CI checkout's proof path is too long.
        env['XDG_RUNTIME_DIR'] = str(runtime)
        server_env = env.copy()
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
        pactl_env = {**server_env, 'LC_ALL': 'C'}

        def select_private_sink():
            assert all(process.poll() is None for process in (core, pulse, policy)), (
                'private audio service exited before selecting the sink')
            selected = subprocess.run(['pactl', 'set-default-sink', 'cumulus_private'],
                                      env=pactl_env, capture_output=True, text=True, timeout=3)
            if selected.returncode == 0:
                return True
            # Pulse accepts commands before WirePlumber publishes the default
            # metadata. Retry only that startup response in this private sink.
            error = selected.stderr.strip()
            if selected.returncode == 1 and error == 'Failure: Not supported':
                return False
            raise RuntimeError(f'cannot select private sink ({selected.returncode}): {error}')

        wait_for('private sink selection (WirePlumber default metadata)', select_private_sink, 10)

        def private_sink_published():
            assert all(process.poll() is None for process in (core, pulse, policy)), (
                'private audio service exited before publishing the selected sink')
            actual_sink = subprocess.check_output(['pactl', 'get-default-sink'], env=pactl_env,
                                                  text=True, timeout=3).strip()
            # The successful setter can precede publication to this separate
            # Pulse connection. Wait for metadata without setting it again.
            if actual_sink == '@DEFAULT_SINK@':
                return False
            assert actual_sink == 'cumulus_private', f'wrong private default sink: {actual_sink}'
            return True

        wait_for('private sink readback (WirePlumber default metadata)', private_sink_published, 10)
        wm = spawn('openbox', '--sm-disable', process_env=env)

        def wm_ready():
            assert wm.poll() is None, 'private window manager exited'
            return 'window id #' in subprocess.check_output(['xprop', '-root', '_NET_SUPPORTING_WM_CHECK'], text=True, timeout=3)

        wait_for('private window manager', wm_ready)
        media = private/'music_fixture'
        media.mkdir()
        source = media/'Synthetic.flac'
        # Keep 440 Hz dominant for the GEQ proof, with quieter test tones in
        # the bass and treble shelves. All three remain below clipping.
        fixture = '+'.join(f'{gain}*sin(2*PI*{frequency}*t)'
                           for frequency, gain in zip(MONITOR_TONES, (.02, .10, .02)))
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi', '-i',
                        f'aevalsrc={fixture}:s=48000:d=65',
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


def start_accessibility():
    # Activated services inherit the D-Bus daemon's environment, so clear
    # caller overrides before creating the private session, not just here.
    for key in ('AT_SPI_BUS_ADDRESS', 'AT_SPI_DISPLAY', 'NO_AT_BRIDGE'):
        assert key not in os.environ, f'unset {key} before starting the private D-Bus session'
    import gi
    gi.require_version('Atspi', '2.0')
    from gi.repository import Atspi

    # Start the private session's accessibility bus before launching the app,
    # so the ordinary GTK/WebKit process exposes its accessibility tree.
    assert Atspi.init() == 0, 'private AT-SPI session could not initialize'
    Atspi.set_timeout(1500, 1500)
    return Atspi


def focus_installed_control(atspi, app, name, roles, dialog=None, *, reveal_sidebar=False):
    from gi.repository import GLib

    assert not (dialog and reveal_sidebar), 'sidebar reveal must target main navigation'

    def children(node):
        for index in range(node.get_child_count()):
            child = node.get_child_at_index(index)
            if child is not None:
                yield child

    def matching(root, label, allowed, *, require_showing=True):
        pending = list(children(root))
        visited = 0
        while pending:
            node = pending.pop()
            visited += 1
            assert visited <= 512, 'unexpectedly large installed accessibility subtree'
            if node.get_role() in allowed and node.get_name() == label:
                if not require_showing or node.get_state_set().contains(atspi.StateType.SHOWING):
                    yield node
            else:
                pending.extend(children(node))

    def find_live_control():
        desktop = atspi.get_desktop(0)
        if desktop is None:
            return None
        for application in children(desktop):
            if application.get_process_id() != app.pid:
                continue
            # WebKit controls themselves belong to a subprocess. Scope them
            # through the ordinary package's own top-level application.
            root = application
            viewport = None
            if reveal_sidebar:
                landmark = (atspi.Role.LANDMARK,)
                sidebars = list(matching(root, '主导航', landmark))
                assert len(sidebars) <= 1, 'multiple installed main sidebars'
                if not sidebars:
                    return None
                groups = list(matching(sidebars[0], '我的', landmark, require_showing=False))
                assert len(groups) <= 1, 'multiple installed personal navigation groups'
                if not groups:
                    return None
                root = groups[0]
                viewport = root.get_parent()
                assert viewport is not None, 'installed personal navigation has no scroll viewport'
            elif dialog:
                dialogs = list(matching(root, dialog, (atspi.Role.DIALOG,)))
                assert len(dialogs) <= 1, f'multiple installed dialogs named {dialog!r}'
                if not dialogs:
                    return None
                root = dialogs[0]
            controls = list(matching(root, name, roles, require_showing=not reveal_sidebar))
            assert len(controls) <= 1, f'multiple installed controls named {name!r}'
            if controls and controls[0].get_state_set().contains(atspi.StateType.SENSITIVE):
                return controls[0], viewport
        return None

    def find():
        assert app.poll() is None, f'installed app exited while locating {name!r}'
        try:
            return find_live_control()
        except GLib.Error as error:
            # A route change can replace the WebKit accessibility subtree
            # while a poll is walking it. Rescan the same live app, without
            # treating a transport error or a failed action as a missed poll.
            if (error.domain, error.code, error.message) != (
                    'atspi_error', 0, 'The application no longer exists'):
                raise
            assert app.poll() is None, f'installed app exited while locating {name!r}'
            return None

    control, viewport = wait_for(f'installed control {name!r}', find)
    component = control.get_component_iface()
    assert component is not None, f'installed control {name!r} has no component'
    if viewport is not None:
        viewport_component = viewport.get_component_iface()
        assert viewport_component is not None, 'installed sidebar viewport has no component'
        assert component.scroll_to(atspi.ScrollType.ANYWHERE), f'cannot reveal installed sidebar control {name!r}'

        def visible_in_sidebar():
            if not control.get_state_set().contains(atspi.StateType.SHOWING):
                return False
            bounds = viewport_component.get_extents(atspi.CoordType.SCREEN)
            target = component.get_extents(atspi.CoordType.SCREEN)
            center_x, center_y = target.x + target.width // 2, target.y + target.height // 2
            return (target.width > 0 and target.height > 0
                    and bounds.x <= center_x < bounds.x + bounds.width
                    and bounds.y <= center_y < bounds.y + bounds.height)

        wait_for(f'visible installed sidebar control {name!r}', visible_in_sidebar)
    assert component.grab_focus(), f'cannot focus installed control {name!r}'
    wait_for(f'keyboard focus on {name!r}',
             lambda: control.get_state_set().contains(atspi.StateType.FOCUSED))
    if viewport is not None:
        assert visible_in_sidebar(), f'installed sidebar control {name!r} moved out of view after focus'


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


def sink_tone(env):
    """Capture one second from the private sink without retaining raw PCM."""
    assert shutil.which('parec'), 'monitor proof requires parec from the PulseAudio utilities'
    expected_bytes = 48000 * 2
    raw = bytearray()
    deadline = time.monotonic() + 8
    process = subprocess.Popen(
        ['parec', '--device=cumulus_private.monitor', '--rate=48000',
         '--channels=1', '--format=s16le', '--raw'],
        env=env, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    try:
        with process.stdout as stream:
            while len(raw) < expected_bytes:
                remaining = deadline - time.monotonic()
                if remaining <= 0 or not select.select([stream], [], [], remaining)[0]:
                    raise AssertionError('private sink monitor did not produce a full capture')
                chunk = os.read(stream.fileno(), expected_bytes - len(raw))
                if not chunk:
                    raise AssertionError('private sink monitor closed before a full capture')
                raw.extend(chunk)
    finally:
        stop(process)
    # Analyze the central half second, omitting the stream's startup transient.
    pcm = tuple(value for (value,) in struct.iter_unpack('<h', raw[24000:72000]))
    rms = math.sqrt(math.fsum(value * value for value in pcm) / len(pcm))
    result = {'rms': round(rms, 3)}
    for frequency in MONITOR_TONES:
        omega = math.tau * frequency / 48000
        sine = math.fsum(value * math.sin(omega * index) for index, value in enumerate(pcm))
        cosine = math.fsum(value * math.cos(omega * index) for index, value in enumerate(pcm))
        result[f'amplitude{frequency}Hz'] = round(2 * math.hypot(sine, cosine) / len(pcm), 3)
    return result


def verify_playback(root, app, window, x, y, width, height, source, env, screenshot, packaged_filter=None, verify_monitor_pcm=False, accessibility=None):
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

    def equalizer_preferences():
        with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True, timeout=.5)) as db:
            row = db.execute("SELECT value FROM state WHERE owner='local-guest' "
                             "AND kind='equalizer-preferences-v1' AND key='current'").fetchone()
        return json.loads(row[0]) if row else None

    def preset_saved(bypassed=False, enabled=True, boost_bass=False, boost_treble=False):
        if value := equalizer_preferences():
            settings = value['settings']
            bass, treble = settings.get('bass_db', 0), settings.get('treble_db', 0)
            return (value['preset'] == {'kind': 'built_in', 'index': 1}
                    and settings['enabled'] is enabled and any(settings['gains'])
                    and (bass >= 6 if boost_bass else bass == 0)
                    and (treble >= 6 if boost_treble else treble == 0)
                    and settings.get('tone_enabled', False) is (boost_bass or boost_treble)
                    and settings.get('master_bypassed', False) is bypassed)
        return False

    def committed_progress(label):
        # A previous stage's position may already be stale by a full PCM
        # capture. Anchor to this commit, then let queued audio drain before
        # recording the next stage. The position check still proves the same
        # native selection is advancing rather than hiding a stalled output.
        anchor = wait_for(f'{label}: position at commit', lambda: position('playing', 0))
        assert anchor['entry_id'] == first['entry_id']
        settled = wait_for(f'{label}: post-commit advancement',
                           lambda: position('playing', anchor['seconds'] + 1))
        assert settled['entry_id'] == first['entry_id']
        return {'atCommit': anchor['seconds'], 'beforeCapture': settled['seconds']}

    monitor = {}
    observations = {}

    def capture_tone(stage, timing=None):
        result = sink_tone(env)
        monitor[stage] = result
        observations[stage] = {'tone': result, 'nativePosition': timing}
        # Preserve the last actual measurement even when an assertion fails,
        # without retaining PCM or recording anything outside the private sink.
        (root/'monitor-pcm-observations.json').write_text(
            json.dumps(observations, indent=2) + '\n', encoding='utf-8')
        return result

    with closing(Input(x, y)) as inputs:
        assert accessibility is not None, 'installed playback proof requires a private AT-SPI session'

        def activate(name, roles, *, reveal_sidebar=False):
            focus_installed_control(accessibility, app, name, roles, reveal_sidebar=reveal_sidebar)
            assert active_window() == window, f'installed app lost keyboard focus for {name!r}'
            inputs.key('Return')

        button_roles = (accessibility.Role.PUSH_BUTTON,)
        wait_for('installed main window focus', lambda: active_window() == window)
        activate('本地音乐', (*button_roles, accessibility.Role.LINK), reveal_sidebar=True)
        focus_installed_control(accessibility, app, '选择音乐目录', button_roles)
        screenshot('installed-local-page.png')
        assert active_window() == window, 'installed app lost keyboard focus before opening the picker'
        inputs.key('Return')
        wait_for('native file picker focus', lambda: active_window() not in (None, window))
        inputs.key('l', 'Control_L')
        time.sleep(.35)
        for char in str(source.parent):
            key, modifier = {
                '/': ('slash', None), '-': ('minus', None), '_': ('minus', 'Shift_L')
            }.get(char, (char, None))
            inputs.key(key, modifier)
        time.sleep(.5)  # GTK resolves the typed location before activation.
        inputs.key('Return')
        wait_for('installed main window focus after selection', lambda: active_window() == window)
        local_id = wait_for('generated FLAC indexed by the installed app', indexed)
        time.sleep(1.7)  # Allow one WebView local-index refresh after the native commit.
        screenshot('installed-local-imported.png')
        activate('播放全部', button_roles)
        first = wait_for('native playing position', lambda: position('playing', .15))
        next_position = wait_for('native position advancement', lambda: position('playing', first['seconds']+1))
        assert next_position['entry_id'] == first['entry_id']
        wait_for('installed app linked to private PipeWire sink', lambda: private_pipewire_stream(app.pid, env), 6)
        screenshot('installed-local-playing.png')
        if verify_monitor_pcm:
            baseline = capture_tone('playing')
            assert baseline['rms'] > 50
            assert baseline['amplitude440Hz'] > baseline['rms'] * 1.3, 'private sink did not receive the 440 Hz fixture'
        if packaged_filter:
            inputs.click(1049, 722)  # EQ in the playback footer.
            def equalizer_key(name, roles, key):
                focus_installed_control(accessibility, app, name, roles, dialog='均衡器')
                assert active_window() == window, f'installed app lost keyboard focus for {name!r}'
                inputs.key(key)

            equalizer_key('均衡器预设', (accessibility.Role.COMBO_BOX,), 'Down')
            master = '启用均衡器和低高音增强'
            # WebKit exposes this ARIA switch as a toggle button. Ubuntu
            # 24.04's AT-SPI bindings do not define the newer SWITCH role.
            master_roles = (accessibility.Role.TOGGLE_BUTTON,)
            if (switch_role := getattr(accessibility.Role, 'SWITCH', None)) is not None:
                master_roles += (switch_role,)
            wait_for('installed equalizer preset committed', preset_saved)
            wait_for('installed audio filter mapped by the player',
                     lambda: str(packaged_filter) in Path(f'/proc/{app.pid}/maps').read_text())
            equalized_progress = committed_progress('installed equalizer preset')
            screenshot('installed-local-equalizer.png')
            if verify_monitor_pcm:
                capture_tone('equalized', equalized_progress)
                assert monitor['equalized']['amplitude440Hz'] > baseline['amplitude440Hz'] * 1.05, (
                    'installed preset did not increase the 440 Hz tone')
                equalizer_key(master, master_roles, 'space')
                wait_for('installed master bypass committed', lambda: preset_saved(True))
                bypassed_progress = committed_progress('installed master bypass')
                capture_tone('bypassed', bypassed_progress)
                assert math.isclose(monitor['bypassed']['amplitude440Hz'],
                                    baseline['amplitude440Hz'], rel_tol=.03), (
                    'installed master bypass did not restore the original 440 Hz tone')
                equalizer_key(master, master_roles, 'space')
                wait_for('installed master reactivation committed', preset_saved)
                restored_progress = committed_progress('installed master reactivation')
                capture_tone('restored', restored_progress)
                assert math.isclose(monitor['restored']['amplitude440Hz'],
                                    monitor['equalized']['amplitude440Hz'], rel_tol=.03), (
                    'reactivating the installed preset did not restore the equalized 440 Hz tone')
                equalizer_key('启用均衡器', master_roles, 'space')
                wait_for('installed GEQ disabled', lambda: preset_saved(enabled=False))
                sliders = (accessibility.Role.SLIDER,)
                equalizer_key('低音增益', sliders, 'End')
                wait_for('installed bass adjustment committed', lambda: preset_saved(enabled=False, boost_bass=True))
                equalizer_key('高音增益', sliders, 'End')
                wait_for('installed treble adjustment committed',
                         lambda: preset_saved(enabled=False, boost_bass=True, boost_treble=True))
                tone = capture_tone('toneOnly', committed_progress('installed bass and treble'))
                for frequency in (60, 10000):
                    key = f'amplitude{frequency}Hz'
                    assert baseline[key] > 25 and tone[key] > baseline[key] * 1.5, (
                        f'installed tone controls did not increase {frequency} Hz with GEQ off')
                equalizer_key('重置低音和高音', button_roles, 'Return')
                wait_for('installed tone reset committed', lambda: preset_saved(enabled=False))
                reset = capture_tone('toneReset', committed_progress('installed tone reset'))
                for frequency in MONITOR_TONES:
                    key = f'amplitude{frequency}Hz'
                    assert math.isclose(reset[key], baseline[key], rel_tol=.03), (
                        f'installed tone reset did not restore the original {frequency} Hz tone')

                before_manual = equalizer_preferences()['settings']['gains']
                assert before_manual[4] == 4, 'expected the 500 Hz band of the Pop preset'
                manual_curve = [*before_manual]
                manual_curve[4] = 12

                def curve_saved(preset, gains, *, enabled=True, custom=False):
                    value = equalizer_preferences()
                    if not value:
                        return False
                    settings = value['settings']
                    saved = value.get('saved_custom') or {}
                    return (value['preset'] == preset and settings['enabled'] is enabled
                            and not settings.get('master_bypassed', False)
                            and settings['gains'] == gains
                            and (not custom or saved.get('gains') == manual_curve))

                equalizer_key('500 Hz 增益', sliders, 'End')
                wait_for('installed 500 Hz band at +12 dB',
                         lambda: curve_saved({'kind': 'manual'}, manual_curve))
                manual = capture_tone('manualBand', committed_progress('installed 500 Hz band change'))
                assert manual['amplitude440Hz'] > monitor['equalized']['amplitude440Hz'] * 1.1, (
                    'changing the 500 Hz band did not increase the nearby 440 Hz tone')
                equalizer_key('保存自定义预设', button_roles, 'Return')
                wait_for('installed custom curve saved',
                         lambda: curve_saved({'kind': 'saved'}, manual_curve, custom=True))
                screenshot('installed-local-custom-equalizer.png')
                equalizer_key('恢复平直', button_roles, 'Return')
                wait_for('installed GEQ reset to flat',
                         lambda: curve_saved({'kind': 'built_in', 'index': 0}, [0] * 10, custom=True))
                flat = capture_tone('flat', committed_progress('installed GEQ flat reset'))
                assert math.isclose(flat['amplitude440Hz'], baseline['amplitude440Hz'], rel_tol=.03), (
                    'flat GEQ did not restore the original 440 Hz tone')
                # The saved slot is the first selectable item; the active
                # flat built-in preset comes after it.
                equalizer_key('均衡器预设', (accessibility.Role.COMBO_BOX,), 'Home')
                wait_for('installed saved curve reselected',
                         lambda: curve_saved({'kind': 'saved'}, manual_curve, custom=True))
                recalled = capture_tone('savedCurve', committed_progress('installed saved curve recall'))
                assert math.isclose(recalled['amplitude440Hz'], manual['amplitude440Hz'], rel_tol=.03), (
                    'recalling the saved curve did not restore its 440 Hz gain')
            inputs.key('Escape')
            time.sleep(.35)
        activate('暂停', button_roles)
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
        if verify_monitor_pcm:
            capture_tone('paused')
            assert monitor['paused']['rms'] < baseline['rms'] * .01, 'private sink was not silent after pause'
    report = {'nativePositionAdvanced': True, 'pauseHeld': True, 'syntheticLocalSource': True,
            'privatePipeWireNullSink': True, 'physicalAudioOutputVerified': False,
            'firstSeconds': first['seconds'], 'laterSeconds': next_position['seconds'],
            'pausedSeconds': held['seconds'],
            'sourceSha256': hashlib.sha256(source.read_bytes()).hexdigest()}
    if verify_monitor_pcm:
        report['privateSinkMonitorPcm'] = monitor
        if packaged_filter:
            report['masterBypassRestoredUnfilteredPcm'] = True
            report['masterReactivationRestoredEqualizedPcm'] = True
            report['independentToneAndResetVerifiedInPcm'] = True
            report['manualBandAndSavedCurveVerifiedInPcm'] = True
    if packaged_filter:
        report.update(equalizerPresetCommitted=True, installedFilterMapped=True,
                      sameTrackAdvancedWithEqualizer=True)
    return report
