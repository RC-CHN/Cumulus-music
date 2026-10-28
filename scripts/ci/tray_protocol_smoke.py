"""Observe a real Tauri/libappindicator tray over an isolated D-Bus watcher.

Default mode validates only native protocol callbacks; --lifecycle validates
the production close prompt and recovery. Neither proves a physical panel
icon, native media-key controls or the Windows shell.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import time
import gi
gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib

WATCHER = 'org.kde.StatusNotifierWatcher'
WATCHER_PATH = '/StatusNotifierWatcher'
XML = '''<node><interface name="org.kde.StatusNotifierWatcher">
<method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method>
<method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method>
<property name="ProtocolVersion" type="i" access="read"/>
<property name="RegisteredStatusNotifierItems" type="as" access="read"/>
<property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
<signal name="StatusNotifierItemRegistered"><arg type="s"/></signal>
<signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal>
<signal name="StatusNotifierHostRegistered"/>
<signal name="StatusNotifierHostUnregistered"/>
</interface></node>'''

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--lifecycle', action='store_true', help='drive production window lifecycle and UI')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    # The caller must use a private dbus-run-session; never replace a real tray.
    connection = Gio.bus_get_sync(Gio.BusType.SESSION, None)
    exists = connection.call_sync('org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'NameHasOwner', GLib.Variant('(s)', (WATCHER,)), None, Gio.DBusCallFlags.NONE, 1000, None).unpack()[0]
    assert not exists, 'refuse to replace an existing status notifier watcher'
    registrations, items = [], []
    state = {'host': False, 'owned': False}
    def method(conn, sender, path, interface, name, parameters, invocation):
        argument = parameters.unpack()[0]
        if name == 'RegisterStatusNotifierItem':
            service, item_path = (sender, argument) if argument.startswith('/') else (argument, '/StatusNotifierItem')
            item = {'service': service, 'path': item_path, 'argument': argument, 'sender': sender}
            registrations.append(item)
            if item not in items:
                items.append(item)
            invocation.return_value(GLib.Variant('()', ()))
            conn.emit_signal(None, WATCHER_PATH, WATCHER, 'StatusNotifierItemRegistered', GLib.Variant('(s)', (service + item_path,)))
        else:
            invocation.return_value(GLib.Variant('()', ()))
    def prop(conn, sender, path, interface, name):
        if name == 'ProtocolVersion': return GLib.Variant('i', 0)
        if name == 'RegisteredStatusNotifierItems': return GLib.Variant('as', [v['service'] + v['path'] for v in items])
        if name == 'IsStatusNotifierHostRegistered': return GLib.Variant('b', state['host'])
    node = Gio.DBusNodeInfo.new_for_xml(XML)
    registration = connection.register_object(WATCHER_PATH, node.interfaces[0], method, prop, None)
    def own():
        return Gio.bus_own_name_on_connection(connection, WATCHER, Gio.BusNameOwnerFlags.NONE,
            lambda *_: state.update(owned=True), lambda *_: state.update(owned=False))
    owner = own()
    context = GLib.MainContext.default()
    seen = set()
    if args.screenshots:
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
    def wait(predicate, label, seconds=30):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            while context.pending(): context.iteration(False)
            if args.screenshots:
                for evidence in (root / 'run').glob('ux_*.json'):
                    if evidence in seen: continue
                    window = Gdk.get_default_root_window()
                    picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                    assert picture is not None
                    picture.savev(str(evidence.with_suffix('.png')), 'png', [], [])
                    seen.add(evidence)
            failure = root / 'run/failed.json'
            assert not failure.exists(), failure.read_text() if failure.exists() else ''
            value = predicate()
            if value: return value
            time.sleep(.02)
        raise AssertionError('timed out: ' + label)
    wait(lambda: state['owned'], 'private watcher name')
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')]:
        folder = root / name
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    process = None
    try:
        with (root / 'host.log').open('x') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
            wait(lambda: (root / 'run/ready.json').exists(), 'tray constructed')
            ready = json.loads((root / 'run/ready.json').read_text())
            assert ready['trayBuilt'] and ready['visible'] and not state['host']
            if args.lifecycle:
                wait(lambda: (root / 'run/host_enable.json').exists(), 'UI unavailable-host checks')
            state['host'] = True
            connection.emit_signal(None, WATCHER_PATH, WATCHER, 'StatusNotifierHostRegistered', None)
            wait(lambda: items, 'native tray item registration')
            def call(item, path, interface, method, parameters):
                return connection.call_sync(item['service'], path, interface, method, parameters, None, Gio.DBusCallFlags.NONE, 3000, None).unpack()
            def menu(item):
                props = call(item, item['path'], 'org.freedesktop.DBus.Properties', 'GetAll', GLib.Variant('(s)', ('org.kde.StatusNotifierItem',)))[0]
                menu_path = props['Menu']
                layout = call(item, menu_path, 'com.canonical.dbusmenu', 'GetLayout', GLib.Variant('(iias)', (0, -1, ['label', 'enabled', 'visible'])))[1]
                labels = {}
                def visit(node):
                    item_id, properties, children = node
                    if 'label' in properties: labels[properties['label'].replace('_', '')] = item_id
                    for child in children: visit(child.unpack() if isinstance(child, GLib.Variant) else child)
                visit(layout)
                return menu_path, labels, props
            item = items[-1]
            menu_path, labels, properties = menu(item)
            def activate(label, event):
                call(item, menu_path, 'com.canonical.dbusmenu', 'Event', GLib.Variant('(isvu)', (labels[label], 'clicked', GLib.Variant('i', 0), 0)))
                path = root / 'run' / (event + '.json')
                wait(path.exists, 'real native menu callback ' + event)
                result = json.loads(path.read_text())
                assert result['ok'] and result['noAccountOrMediaInitialized']
                return result
            if args.lifecycle:
                wait(lambda: (root / 'run/hidden_first.json').exists(), 'UI first close to tray')
                first = json.loads((root / 'run/hidden_first.json').read_text())
                assert not first['visible'] and first['lifecycle']['hidden'] and first['preferences']['settings']['rememberChoice']
                assert first['backgroundPlayback']['verified'] and first['backgroundPlayback']['advance'] > .2
                call(item, menu_path, 'com.canonical.dbusmenu', 'Event', GLib.Variant('(isvu)', (labels['显示 Cumulus'], 'clicked', GLib.Variant('i', 0), 0)))
                wait(lambda: (root / 'run/hidden_second.json').exists(), 'remembered native close')
                second = json.loads((root / 'run/hidden_second.json').read_text())
                assert not second['visible'] and second['lifecycle']['prompt'] is None
                state['host'] = False
                connection.emit_signal(None, WATCHER_PATH, WATCHER, 'StatusNotifierHostUnregistered', None)
                wait(lambda: (root / 'run/host_restart.json').exists(), 'lost host recovery')
                restored = json.loads((root / 'run/host_restart.json').read_text())
                assert restored['visible'] and not restored['lifecycle']['trayAvailable']
                before = len(registrations)
                Gio.bus_unown_name(owner)
                owner = None
                items.clear()
                until = time.monotonic() + .7
                while time.monotonic() < until:
                    while context.pending(): context.iteration(False)
                    time.sleep(.02)
                state['host'] = True
                owner = own()
                wait(lambda: state['owned'] and len(registrations) > before, 'native registration after restart')
                wait(lambda: (root / 'run/complete.json').exists(), 'remaining WebView checks')
                wait(lambda: process.poll() is not None, 'explicit UI exit')
                assert process.returncode == 0 and (root / 'run/exited.json').exists()
                complete = json.loads((root / 'run/complete.json').read_text())
                assert complete['ok'] and complete['states'] == 14
                assert not args.screenshots or len(seen) == 14
                with sqlite3.connect((root / 'run/state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
                    saved = json.loads(db.execute("SELECT value FROM state WHERE owner='local-device' AND kind='window-behavior-preferences-v1' AND key='current'").fetchone()[0])
                    assert saved['settings'] == {'closeBehavior': 'exit', 'rememberChoice': True}
                    assert db.execute("SELECT count(*) FROM state WHERE kind='window-behavior-recovery-v1'").fetchone()[0] == 1
                    assert db.execute("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name='reject_window'").fetchone()[0] == 0
                report = {'ok': True, 'actualWebView': True, 'actualTauriTray': True,
                    'syntheticPanelHost': True, 'productClosePolicyVerified': True,
                    'realMusicRequests': 0, 'states': 14, 'screenshots': len(seen),
                    'nativeShowMenu': True, 'hostLossRestoresWindow': True, 'backgroundPlayback': first['backgroundPlayback'],
                    'panelRestartReRegisters': True, 'explicitExit': True,
                    'physicalPanelIconVerified': False, 'windowsShellVerified': False}
                (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
                print(json.dumps(report))
                return
            hidden = activate('隐藏窗口', 'proof-hide')
            assert not hidden['visible']
            shown = activate('显示 Cumulus', 'proof-show')
            assert shown['visible']
            before = len(registrations)
            Gio.bus_unown_name(owner)
            owner = None
            items.clear()
            # Drain owner-loss events before requesting the same well-known name.
            until = time.monotonic() + .7
            while time.monotonic() < until:
                while context.pending(): context.iteration(False)
                time.sleep(.02)
            owner = own()
            wait(lambda: state['owned'] and len(registrations) > before, 'registration after panel restart')
            item = items[-1]
            menu_path, labels, _ = menu(item)
            done = activate('退出验证', 'proof-quit')
            assert process.wait(timeout=5) == 0
            report = {'ok': True, 'actualTauriTray': True, 'syntheticPanelHost': True,
                'buildSucceededWithoutHost': True, 'nativeMenuHideAndShow': True,
                'reregisteredAfterWatcherRestart': True, 'events': done['events'],
                'registrations': registrations, 'itemProperties': {k: properties.get(k) for k in ['Id', 'Status', 'Menu', 'IconName', 'Title']},
                'physicalPanelIconVerified': False, 'productClosePolicyVerified': False,
                'remoteMusicRequests': 0}
            (root / 'report.json').write_text(json.dumps(report, ensure_ascii=False, indent=2) + '\n')
            print(json.dumps({k: v for k, v in report.items() if k not in ['registrations', 'itemProperties']}))
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait()
        if owner is not None: Gio.bus_unown_name(owner)
        connection.unregister_object(registration)

if __name__ == '__main__':
    main()
