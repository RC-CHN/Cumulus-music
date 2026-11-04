"""Drive the ordinary Linux app's automatic window mode with a private tray."""
from contextlib import ExitStack, closing, contextmanager
import json
import os
from pathlib import Path
import re
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
    from gi.repository import Gdk, GLib

    access = start_accessibility()
    env.pop('CUMULUS_PROOF_DIR')
    env['CUMULUS_DATA_DIR'] = str(root / 'state')
    env['HOME'] = str(root / 'home')
    Path(env['HOME']).mkdir(mode=0o700)
    env['GTK_USE_PORTAL'] = '0'
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

    def radio(app, label):
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
                if node.get_role() == access.Role.RADIO_BUTTON and node.get_name() == label:
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

    def choose(app, inputs, window, label, expected):
        focus_installed_control(access, app, '打开设置', (access.Role.PUSH_BUTTON, access.Role.TOGGLE_BUTTON))
        assert_focused(window)
        inputs.key('Return')
        control = wait_for('startup setting in actual WebView', lambda: radio(app, label))
        component = control.get_component_iface()
        assert component is not None and component.scroll_to(access.ScrollType.ANYWHERE), 'cannot reveal startup radio'
        focus_installed_control(access, app, label, (access.Role.RADIO_BUTTON,))
        assert_focused(window)
        inputs.key('space')
        def saved():
            record = setting()
            return record if record and record['settings']['startupMode'] == expected else None
        record = wait_for('startup setting stored by native IPC', saved)
        def checked():
            control = radio(app, label)
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

    @contextmanager
    def launch(name, *, automatic, host):
        assert not list(items), 'stale fixture tray registration'
        watcher['host'] = host
        with (root / f'{name}.log').open('x') as log:
            child = subprocess.Popen([str(args.binary.resolve()), *(['--autostart'] if automatic else [])],
                                     env=env, stdout=log, stderr=log)
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
            states['frontRevision'] = choose(child, inputs, window, '前台显示', 'front')
            finish(child)

        with launch('automatic-front', automatic=True, host=True) as child:
            window = wait(lambda: one_window(child, visible=True), 'automatic front launch visible')
            states['minisizeRevision'] = choose(child, inputs, window, '最小化显示', 'minisize')
            assert states['minisizeRevision'] > states['frontRevision']
            finish(child)

        with launch('automatic-minisize', automatic=True, host=True) as child:
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
            wait(lambda: one_window(child, visible=True), 'explicit launch ignores saved minisize')
            states['explicitOpenWithSavedMinisize'] = True
            finish(child)

    report = dict(ok=True, productionBinary=True, actualWebView=True, actualTauriTray=True,
                  syntheticPanelHost=True, isolatedLoopbackOnly=True, successfulMusicServiceRequests=0,
                  windowsShellVerified=False, physicalPanelVerified=False, **states)
    (root / 'startup-report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))
