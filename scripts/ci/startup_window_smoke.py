"""Drive the ordinary Linux app's automatic window mode with a private tray."""
from contextlib import ExitStack, closing, contextmanager
import json
import os
from pathlib import Path
import re
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time

from installed_linux_audio import Input, focus_installed_control, start_accessibility, stop, wait_for


def validate_environment(args):
    assert args.outer_network and re.fullmatch(r'net:\[[0-9]+\]', args.outer_network)
    assert os.readlink('/proc/self/ns/net') != args.outer_network
    assert {name for _, name in socket.if_nameindex()} == {'lo'}, 'refuse ordinary launch on an egress-capable host'
    assert re.fullmatch(r':[0-9]+', os.environ['DISPLAY']) and os.environ['GDK_BACKEND'] == 'x11'
    assert os.environ.get('XAUTHORITY'), 'require a private authenticated X11 display'
    assert Path(os.environ['XAUTHORITY']).is_file()


def startup(args, root, env, items, watcher, wait, call, menu):
    import gi
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gdk, Gio, GLib

    access = start_accessibility()
    env.pop('CUMULUS_PROOF_DIR')
    env['CUMULUS_DATA_DIR'] = str(root / 'state')
    env['HOME'] = str(root / 'home')
    Path(env['HOME']).mkdir(mode=0o700)
    env['GTK_USE_PORTAL'] = '0'
    system_config = root / 'system-config'
    system_config.mkdir()
    env['XDG_CONFIG_DIRS'] = str(system_config)
    entry = Path(env['XDG_CONFIG_HOME']) / 'autostart/io.cumulus.music.desktop'
    states = {}
    database = root / 'state/core.sqlite3'

    def setting():
        with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True, timeout=.5)) as db:
            row = db.execute("SELECT value FROM state WHERE owner='local-device' AND "
                             "kind='window-behavior-preferences-v1' AND key='current'").fetchone()
        return json.loads(row[0]) if row else None

    def windows(app, *, visible):
        assert app.poll() is None, 'ordinary app exited unexpectedly'
        tree = subprocess.run(['xwininfo', '-root', '-tree'], capture_output=True, text=True, timeout=3)
        if tree.returncode == 1 and 'Bad Window' in tree.stderr and "Can't query window tree" in tree.stderr:
            # The graphics-only preflight can destroy an X child while
            # xwininfo walks the tree. Discard only that raced sample.
            return
        tree.check_returncode()
        for line in tree.stdout.splitlines():
            match = re.match(r'\s*(0x[0-9a-f]+) "Cumulus":', line)
            if not match:
                continue
            window = match[1]
            props = subprocess.run(['xprop', '-id', window, '_NET_WM_PID'],
                                   capture_output=True, text=True, timeout=3)
            if props.returncode or not props.stdout.strip().endswith(f'= {app.pid}'):
                continue
            info = subprocess.run(['xwininfo', '-id', window, '-stats'],
                                  capture_output=True, text=True, timeout=3)
            if not info.returncode and ('Map State: IsViewable' in info.stdout) is visible:
                yield window

    def one_window(app, *, visible):
        found = list(windows(app, visible=visible))
        assert len(found) <= 1, 'unexpected duplicate main windows'
        return next(iter(found), None)

    def assert_focused(window):
        value = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True, timeout=3)
        assert value.strip().endswith(window), 'main window lost native focus'

    def control_named(app, label, role):
        try:
            return find_control(app, label, role)
        except GLib.Error as error:
            # WebKit may replace the accessibility subtree during route
            # changes or between app launches; never hide other errors.
            if (error.domain, error.code, error.message) != (
                    'atspi_error', 0, 'The application no longer exists'):
                raise
            assert app.poll() is None, 'ordinary app exited while locating an accessibility control'
            return None

    def find_control(app, label, role):
        desktop = access.get_desktop(0)
        for i in range(desktop.get_child_count()):
            child = desktop.get_child_at_index(i)
            if child is None or child.get_process_id() != app.pid:
                continue
            pending = [child]
            seen = 0
            while pending:
                node = pending.pop()
                seen += 1
                assert seen <= 1000, 'unexpectedly large startup settings accessibility tree'
                if role is None:
                    text = node.get_text_iface()
                    if node.get_name() == label or (text and access.Text.get_text(text, 0, -1).strip() == label):
                        return node
                elif node.get_role() == role and node.get_name() == label:
                    return node
                for index in range(node.get_child_count()):
                    child = node.get_child_at_index(index)
                    if child is not None:
                        pending.append(child)
        return None

    def screenshot(name):
        if args.screenshots:
            window_root = Gdk.get_default_root_window()
            image = Gdk.pixbuf_get_from_window(window_root, 0, 0, window_root.get_width(), window_root.get_height())
            assert image is not None
            image.savev(str(root / f'{name}.png'), 'png', [], [])

    def shell_painted(window):
        info = subprocess.check_output(['xwininfo', '-id', window, '-stats'], text=True, timeout=3)
        x, y = (int(re.search(rf'Absolute upper-left {axis}:\s+(-?[0-9]+)', info)[1])
                for axis in ('X', 'Y'))
        # The unmodified light-theme shell paints its red Cumulus badge
        # in the upper-left; a uniformly white native window is insufficient.
        image = Gdk.pixbuf_get_from_window(Gdk.get_default_root_window(), x + 8, y + 8, 90, 72)
        if image is None:
            return False
        pixels, stride, channels = image.get_pixels(), image.get_rowstride(), image.get_n_channels()
        red = 0
        for row in range(72):
            for column in range(90):
                offset = row * stride + column * channels
                r, g, b = pixels[offset:offset + 3]
                red += r > 120 and r > 1.4 * g and r > 1.2 * b
        return red >= 25

    def open_settings(app, inputs, window):
        focus_installed_control(access, app, '打开设置', (access.Role.PUSH_BUTTON, access.Role.TOGGLE_BUTTON))
        assert_focused(window)
        inputs.key('Return')

    def reveal_control(app, window, label, role):
        control = wait_for('startup setting in actual WebView', lambda: control_named(app, label, role))
        component = control.get_component_iface()
        assert component is not None and component.scroll_to(access.ScrollType.ANYWHERE), 'cannot reveal startup radio'
        focus_installed_control(access, app, label, (role,))
        assert_focused(window)

    def registration_toggle(app, inputs, window, enabled):
        reveal_control(app, window, '开机自动运行', access.Role.CHECK_BOX)
        inputs.key('space')
        wait_for('native autostart file state', lambda: entry.is_file() is enabled)

        def observed():
            node = control_named(app, '开机自动运行', access.Role.CHECK_BOX)
            return node and node.get_state_set().contains(access.StateType.CHECKED) is enabled
        wait_for('system registration rendered in WebView', observed)
        if args.screenshots:
            screenshot('autostart-enabled' if enabled else 'autostart-disabled')

    def choose(app, inputs, window, label, expected):
        reveal_control(app, window, label, access.Role.RADIO_BUTTON)
        inputs.key('space')
        def saved():
            record = setting()
            return record if record and record['settings']['startupMode'] == expected else None
        record = wait_for('startup setting stored by native IPC', saved)
        def checked():
            control = control_named(app, label, access.Role.RADIO_BUTTON)
            return control and control.get_state_set().contains(access.StateType.CHECKED)
        wait_for('selected native setting', checked)
        assert record['settings']['closeBehavior'] == 'tray' and not record['settings']['rememberChoice']
        assert one_window(app, visible=True) == window, 'changing the setting affected the current window'
        screenshot(f'startup-{expected}')
        return record['revision']

    def activate(label):
        item = items[-1]
        native_path, labels, _ = menu(item)
        call(item, native_path, 'com.canonical.dbusmenu', 'Event',
             GLib.Variant('(isvu)', (labels[label], 'clicked', GLib.Variant('i', 0), 0)))

    class DesktopChild:
        def __init__(self, pid):
            self.pid, self.returncode = pid, None

        def poll(self):
            if self.returncode is None:
                pid, status = os.waitpid(self.pid, os.WNOHANG)
                if pid:
                    self.returncode = os.waitstatus_to_exitcode(status)
                    GLib.spawn_close_pid(self.pid)
            return self.returncode

        def wait(self, timeout):
            deadline = time.monotonic() + timeout
            while time.monotonic() < deadline:
                if self.poll() is not None:
                    return self.returncode
                time.sleep(.02)
            raise subprocess.TimeoutExpired('XDG autostart', timeout)

        def terminate(self):
            os.kill(self.pid, signal.SIGTERM)

        def kill(self):
            os.kill(self.pid, signal.SIGKILL)

    def launch_registered():
        desktop = Gio.DesktopAppInfo.new_from_filename(str(entry))
        assert desktop and desktop.get_commandline().endswith('" --autostart')
        context = Gio.AppLaunchContext()
        for key in os.environ.keys() - env.keys():
            context.unsetenv(key)
        for key, value in env.items():
            context.setenv(key, value)
        pids = []
        assert desktop.launch_uris_as_manager([], context, GLib.SpawnFlags.DO_NOT_REAP_CHILD,
            None, None, lambda _app, pid, *_: pids.append(pid), None)
        pid, = pids
        child = DesktopChild(pid)
        try:
            # GDesktopAppInfo's descriptive get_executable can truncate a
            # quoted name at its first space; verify the actual spawned process.
            assert Path(f'/proc/{pid}/exe').resolve(strict=True) == args.binary.resolve(strict=True)
            assert Path(f'/proc/{pid}/cmdline').read_bytes().split(b'\0') == [
                os.fsencode(args.binary.resolve()), b'--autostart', b'']
        except Exception:
            stop(child)
            raise
        return child

    @contextmanager
    def launch(name, *, automatic, host, registered=False):
        assert not list(items), 'stale fixture tray registration'
        watcher['host'] = host
        with (root / f'{name}.log').open('x') as log:
            child = (launch_registered() if registered else
                     subprocess.Popen([str(args.binary.resolve()), *(['--autostart'] if automatic else [])],
                                      env=env, stdout=log, stderr=log))
            try:
                yield child
            except Exception:
                screenshot(f'{name}-failed')
                raise
            finally:
                stop(child)
                items.clear()

    def require_hidden_after_deadline(child):
        until = time.monotonic() + 3.6
        def hidden():
            assert not one_window(child, visible=True), 'minisize mapped before restoration'
            return time.monotonic() >= until
        # Continue serving the private DBus watcher's GetAll calls while
        # crossing the product's timeout; a sleeping fixture would simulate loss.
        wait(hidden, 'retained native hide beyond no-host fallback', 5)

    with ExitStack() as stack:
        runtime = Path(stack.enter_context(tempfile.TemporaryDirectory(prefix='cumulus-startup-proof-')))
        env['XDG_RUNTIME_DIR'] = str(runtime)
        env['DBUS_SYSTEM_BUS_ADDRESS'] = f'unix:path={runtime}/no-system-bus'
        wm_log = stack.enter_context((root / 'openbox.log').open('x'))
        wm = subprocess.Popen(['openbox', '--sm-disable'], env=env, stdout=wm_log, stderr=wm_log)
        stack.callback(stop, wm)
        wait_for('private window manager', lambda: wm.poll() is None and 'window id #' in
                 subprocess.check_output(['xprop', '-root', '_NET_SUPPORTING_WM_CHECK'], text=True, timeout=3))
        inputs = stack.enter_context(closing(Input(0, 0)))
        def finish(child):
            wait(lambda: items, 'native tray registration for quit')
            activate('退出 Cumulus')
            wait(lambda: child.poll() is not None, 'ordinary app quit from native tray')
            assert child.returncode == 0

        with launch('explicit-initial', automatic=False, host=True) as child:
            window = wait(lambda: one_window(child, visible=True), 'explicit launch visible')
            open_settings(child, inputs, window)
            assert not entry.exists(), 'launch created an autostart registration'
            def mode_disabled(process):
                radio = control_named(process, '最小化显示', access.Role.RADIO_BUTTON)
                return radio and not radio.get_state_set().contains(access.StateType.SENSITIVE)
            wait_for('unregistered window mode is disabled', lambda: mode_disabled(child))
            registration_toggle(child, inputs, window, True)
            subprocess.run(['desktop-file-validate', str(entry)], check=True, timeout=4)
            assert entry.read_text(encoding='utf-8').count(' --autostart') == 1
            states['registrationEnabledFromUi'] = True
            states['frontRevision'] = choose(child, inputs, window, '前台显示', 'front')
            finish(child)

        with launch('automatic-front', automatic=True, host=True) as child:
            window = wait(lambda: one_window(child, visible=True), 'automatic front launch visible')
            open_settings(child, inputs, window)
            states['minisizeRevision'] = choose(child, inputs, window, '最小化显示', 'minisize')
            assert states['minisizeRevision'] > states['frontRevision']
            finish(child)

        with launch('automatic-minisize', automatic=True, host=True, registered=True) as child:
            wait(lambda: items, 'automatic-minisize native tray')
            # A hidden GTK toplevel may remain unrealized on X11. When it
            # has an XID, also require that native Show reuses that window.
            window = one_window(child, visible=False)
            require_hidden_after_deadline(child)
            activate('显示 Cumulus')
            shown = wait(lambda: one_window(child, visible=True), 'native Show restores window')
            assert window is None or shown == window
            wait_for('native Show focuses window', lambda: subprocess.check_output(
                ['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True, timeout=3).strip().endswith(shown))
            states['showRestoredSameProcess'] = True
            states['launchedThroughRegisteredXdgEntry'] = True
            wait(lambda: shell_painted(shown), 'restored WebView paints its shell', 12)
            focus_installed_control(access, child, '打开设置', (access.Role.PUSH_BUTTON, access.Role.TOGGLE_BUTTON))
            states['restoredWebViewPaintedAndAccessible'] = True
            screenshot('startup-show-restored')
            finish(child)

        with launch('automatic-host-lost', automatic=True, host=True) as child:
            wait(lambda: items, 'native tray before host loss')
            window = one_window(child, visible=False)
            # Allow one known successful monitor observation, then remove the host.
            require_hidden_after_deadline(child)
            watcher['host'] = False
            shown = wait(lambda: one_window(child, visible=True), 'host loss restores the window', 2.5)
            assert window is None or shown == window
            states['hostLossRestoredSameProcess'] = True
            finish(child)

        with launch('automatic-no-host', automatic=True, host=False) as child:
            wait(lambda: items, 'registered item without a host')
            assert not one_window(child, visible=True), 'no-host fallback skipped the bounded tray wait'
            window = one_window(child, visible=False)
            shown = wait(lambda: one_window(child, visible=True), 'bounded no-host fallback', 5)
            assert window is None or shown == window
            states['noHostFallbackRestoredSameProcess'] = True
            finish(child)

        with launch('explicit-saved-minisize', automatic=False, host=True) as child:
            window = wait(lambda: one_window(child, visible=True), 'explicit launch ignores saved minisize')
            states['explicitOpenWithSavedMinisize'] = True
            open_settings(child, inputs, window)
            registration_toggle(child, inputs, window, False)
            wait_for('window choices disabled after unregistration', lambda: mode_disabled(child))
            states['registrationDisabledFromUi'] = True

            foreign = b'[Desktop Entry]\nType=Application\nName=External fixture\nExec=/bin/false\n'
            entry.write_bytes(foreign)
            reveal_control(child, window, '重新读取窗口设置', access.Role.PUSH_BUTTON)
            inputs.key('Return')
            def readback_external():
                button = control_named(child, '开机自动运行', access.Role.CHECK_BOX)
                message = control_named(child, '同名启动项由系统或其他程序管理，Cumulus 未修改。', None)
                return button and message and not button.get_state_set().contains(access.StateType.SENSITIVE)
            wait_for('external registration blocks native write', readback_external)
            assert entry.read_bytes() == foreign
            states['externalRegistrationPreserved'] = True
            screenshot('autostart-external-preserved')
            finish(child)

    report = dict(ok=True, productionBinary=True, actualWebView=True, actualTauriTray=True,
                  syntheticPanelHost=True, isolatedLoopbackOnly=True, successfulMusicServiceRequests=0,
                  windowsShellVerified=False, physicalPanelVerified=False, **states)
    (root / 'startup-report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))
