"""Real Linux WebView folder menu against a private fake FileManager1/xdg-open."""
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
from shutil import which
import subprocess
import sys
import time
from motion_smoke import launch

SERVICE = 'org.freedesktop.FileManager1'
OBJECT = '/org/freedesktop/FileManager1'
XML = '''<node><interface name="org.freedesktop.FileManager1">
<method name="ShowItems"><arg type="as" direction="in"/><arg type="s" direction="in"/></method>
</interface></node>'''


def serve(root, *, exact_successes=None):
    import gi
    gi.require_version('Gio', '2.0')
    from gi.repository import Gio, GLib

    connection = Gio.bus_get_sync(Gio.BusType.SESSION)
    response = connection.call_sync(
        'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus',
        'RequestName', GLib.Variant('(su)', (SERVICE, 4)), GLib.VariantType.new('(u)'),
        Gio.DBusCallFlags.NONE, 2000, None,
    )
    assert response.unpack() == (1,), 'fake FileManager1 must own the isolated name'
    info = Gio.DBusNodeInfo.new_for_xml(XML)
    calls = 0

    def receive(_bus, _sender, path, interface, method, parameters, invocation):
        nonlocal calls
        assert (path, interface, method) == (OBJECT, SERVICE, 'ShowItems')
        calls += 1
        with (root / 'show-items.jsonl').open('a', encoding='utf-8') as output:
            output.write(json.dumps(parameters.unpack()) + '\n')
        if calls <= (exact_successes if exact_successes is not None else 1):
            invocation.return_value(GLib.Variant('()', ()))
        elif exact_successes is not None:
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', 'unexpected retry')
        elif calls == 2:
            invocation.return_dbus_error('org.freedesktop.DBus.Error.UnknownMethod', 'synthetic absence')
        elif calls == 3:
            invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied', 'synthetic rejection')
        elif calls == 4:
            # Genuine no response before the native 2 second deadline. Keep
            # the invocation owned, and deliver a deliberately late success.
            def late_reply():
                invocation.return_value(GLib.Variant('()', ()))
                return False
            GLib.timeout_add(3200, late_reply)
        else:
            invocation.return_dbus_error('org.freedesktop.DBus.Error.Failed', 'unexpected retry')

    assert connection.register_object(OBJECT, info.interfaces[0], receive)
    (root / 'fake-shell.ready').write_text('ready\n', encoding='utf-8')
    GLib.MainLoop().run()


@contextmanager
def fake_file_manager(root, env, *, once=False, exact_successes=None):
    """Start the test service on the caller's private D-Bus session."""
    with (root / 'shell.log').open('x', encoding='utf-8') as log:
        assert not once or exact_successes is None
        count = 1 if once else exact_successes
        argv = (['--serve', str(root)] if count is None
                else ['--serve-exact', str(root), str(count)])
        service = subprocess.Popen(['/usr/bin/python3', __file__, *argv],
                                   env=env, stdout=log, stderr=log)
        try:
            limit = time.monotonic() + 5
            while not (root / 'fake-shell.ready').is_file():
                assert service.poll() is None and time.monotonic() < limit, 'fake shell did not start'
                time.sleep(.05)
            yield
            assert service.poll() is None
        finally:
            service.terminate()
            service.wait(timeout=5)


def verify(binary, root, screenshots):
    root.mkdir(parents=True, exist_ok=False)
    run = root / 'run'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    if (ffmpeg := os.environ.get('CUMULUS_LOCAL_FIXTURE_FFMPEG')):
        env['CUMULUS_LOCAL_FIXTURE_FFMPEG'] = ffmpeg
    env.update(CUMULUS_PROOF_DIR=str(run), CUMULUS_LOCAL_FOLDER_REVEAL_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR'):
        path = root / 'os-profile' / key
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    fixture_bin = root / 'fake-bin'
    fixture_bin.mkdir()
    fake_open = fixture_bin / 'xdg-open'
    fake_open.write_text(
        f'#!{sys.executable}\nimport json, sys\nfrom pathlib import Path\n'
        f'with Path({str(root / "fallback.jsonl")!r}).open("a", encoding="utf-8") as output:\n'
        '    output.write(json.dumps(sys.argv[1:]) + "\\n")\n',
        encoding='utf-8',
    )
    fake_open.chmod(0o700)
    env['PATH'] = str(fixture_bin) + os.pathsep + env.get('PATH', '')
    assert which('xdg-open', path=env['PATH']) == str(fake_open)
    with fake_file_manager(root, env):
        with (root / 'host.log').open('x', encoding='utf-8') as log:
            code = launch(binary, env, log, run, screenshots)
        failed = run / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0
    completed = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
    assert completed['ok'] and completed['focusRestored'] and not completed['selectedFolderChanged']
    assert completed['outcomes'] == ['reveal', 'fallback', 'deny', 'timeout']
    assert completed['negative'] == ['invalid_input', 'snapshot_changed', 'snapshot_changed']
    assert completed['realMusicRequests'] == 0
    def lines(name):
        return [json.loads(line) for line in (root / name).read_text(encoding='utf-8').splitlines()]
    target = (run / 'media/Bonus').resolve()
    assert lines('show-items.jsonl') == [[[target.as_uri()], '']] * 4
    assert lines('fallback.jsonl') == [[str(target.parent)]], 'fallback must occur once, only for absent method'
    stages = {path.stem for path in run.glob('ux_*.png')}
    assert stages == ({'ux_01_folder_menu', 'ux_02_folder_focus', 'ux_03_folder_failure'} if screenshots else set())
    report = {'ok': True, 'actualWebView': True, 'fakeNativeShell': True, 'revealCalls': 4,
              'parentFallbackCalls': 1, 'realFileManagerOpened': False, 'realMusicRequests': 0,
              'screenshots': len(stages)}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def main():
    if len(sys.argv) == 3 and sys.argv[1] == '--serve':
        serve(Path(sys.argv[2]))
        return
    if len(sys.argv) == 4 and sys.argv[1] == '--serve-exact':
        count = int(sys.argv[3])
        assert 1 <= count <= 16
        serve(Path(sys.argv[2]), exact_successes=count)
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--private-child', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    assert sys.platform == 'linux'
    if args.private_child:
        verify(args.binary.resolve(), args.root.resolve(), args.screenshots)
    else:
        # Never acquire FileManager1 or use the fake launcher on a user bus.
        env = {k: v for k, v in os.environ.items()
               if k not in ('DBUS_SESSION_BUS_ADDRESS', 'AT_SPI_BUS_ADDRESS', 'AT_SPI_DISPLAY')}
        subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--private-child',
                        '--binary', str(args.binary.resolve()), '--root', str(args.root.resolve()),
                        *(['--screenshots'] if args.screenshots else [])],
                       env=env, check=True, timeout=195)


if __name__ == '__main__':
    main()
