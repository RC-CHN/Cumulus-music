"""Cold native argv → actual ordinary Linux WebView and pinned decoder."""
import argparse
from contextlib import closing
import json
import os
from pathlib import Path
import re
import socket
import sqlite3
import subprocess
import time

from diagnostics import verify_proof_databases
from installed_linux_audio import (
    Input, focus_installed_control, isolated_audio, private_pipewire_stream, read_playback,
    sink_tone, start_accessibility, stop, wait_for,
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--outer-network', required=True)
    args = parser.parse_args()
    assert re.fullmatch(r'net:\[[0-9]+\]', args.outer_network)
    assert args.outer_network != os.readlink('/proc/self/ns/net')
    assert {name for _, name in socket.if_nameindex()} == {'lo'}, 'run only in a private loopback network'
    assert not Path('/dev/snd').exists(), 'hide host audio devices'
    assert re.fullmatch(r':[0-9]+', os.environ['DISPLAY']) and os.environ['GDK_BACKEND'] == 'x11'
    assert Path(os.environ['XAUTHORITY']).is_file()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_DATA_DIR'] = str(root / 'state')
    env['CUMULUS_DEVICE_IDENTITY'] = 'native'
    for key, suffix in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache')]:
        folder = root / suffix
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    access = start_accessibility()
    import gi
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gdk

    database = root / 'state/core.sqlite3'

    def active(app):
        value = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True, timeout=3)
        match = re.search(r'window id # (0x[0-9a-f]+)', value)
        if not match:
            return None
        owner = subprocess.run(['xprop', '-id', match[1], '_NET_WM_PID'],
                               capture_output=True, text=True, timeout=3)
        return match[1] if not owner.returncode and owner.stdout.strip().endswith(f'= {app.pid}') else None

    def files():
        if not database.is_file():
            return None
        rows = read_playback(database)
        queue = rows.get('queue')
        if queue is None or len(queue['items']) != 2:
            return None
        return queue

    with isolated_audio(root, env) as seed:
        first = root / '1 合成 空格.flac'
        second = root / '2 合成 空格.mp3'
        bad = root / '0 无效音频.flac'
        first.write_bytes(seed.read_bytes())
        bad.write_text('https://example.invalid/never-open.flac\n', encoding='ascii')
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi',
                        '-i', 'sine=frequency=10000:sample_rate=48000:duration=12',
                        '-metadata', 'title=Second Native Open Track', str(second)],
                       check=True, timeout=20)
        with (root / 'ordinary.log').open('x', encoding='utf-8') as log:
            app = subprocess.Popen([str(args.binary.resolve()), '--play', str(bad), str(first), str(second)],
                                   env=env, stdout=log, stderr=log)
            try:
                window = wait_for('ordinary GUI focused', lambda: active(app))
                queue = wait_for('two valid cold launch items in order', files)
                entries = queue['items']
                assert all(i['source'] == {'kind': 'local', 'nativeOpen': True} for i in entries)
                assert queue['current'] == entries[0]['entry_id']
                assert [i['resource']['native_open_file']['path'] for i in entries] == [str(first), str(second)]
                assert entries[0]['resource']['metadata']['format'] == 'flac'
                assert entries[1]['resource']['metadata']['format'] == 'mp3'
                assert entries[1]['resource']['metadata']['title'] == 'Second Native Open Track'
                with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True)) as db:
                    assert db.execute("SELECT count(*) FROM state WHERE kind='library_files_v1'").fetchone()[0] == 0

                def position(state, seconds):
                    assert app.poll() is None
                    value = read_playback(database).get('position')
                    return value if (value and value['entry_id'] == entries[0]['entry_id']
                                     and value['state'] == state and value['seconds'] >= seconds) else None

                first_clock = wait_for('actual pinned native playback advances', lambda: position('playing', .5))
                wait_for('same entry continues', lambda: position('playing', first_clock['seconds'] + .7))
                wait_for('ordinary GUI routed to its private PipeWire null sink',
                         lambda: private_pipewire_stream(app.pid, env))
                tone = sink_tone(env)
                assert tone['amplitude440Hz'] > 200 and tone['rms'] > 200, 'expected synthetic PCM missing'

                with closing(Input(0, 0)) as inputs:
                    focus_installed_control(access, app, '暂停', (access.Role.PUSH_BUTTON,))
                    assert active(app) == window
                    inputs.key('Return')
                    paused = wait_for('real GUI pauses same native entry', lambda: position('paused', 1))
                    time.sleep(1.25)
                    held = position('paused', 1)
                    assert held and abs(held['seconds'] - paused['seconds']) < .06
                    focus_installed_control(access, app, '下一首', (access.Role.PUSH_BUTTON,))
                    assert active(app) == window
                    inputs.key('Return')
                    def second_playing():
                        value = read_playback(database).get('position')
                        return value and value['entry_id'] == entries[1]['entry_id'] and value['state'] == 'playing' and value['seconds'] > .6
                    wait_for('second pinned file actually plays', second_playing)
                    second_tone = sink_tone(env)
                    assert second_tone['amplitude10000Hz'] > 200
                    assert second_tone['amplitude440Hz'] < tone['amplitude440Hz'] * .1

                duplicate = subprocess.run([str(args.binary.resolve())],
                    env=env, capture_output=True, text=True, timeout=25)
                denied = 'already running for this app profile' in duplicate.stderr
                assert duplicate.returncode == 2 and denied, (duplicate.returncode, denied)
                assert read_playback(database)['queue']['current'] == entries[1]['entry_id']
            finally:
                stop(app)

        # The supplied file is invalid, but the previous queue could otherwise
        # autoplay. Write the synthetic device preference only after shutdown.
        with closing(sqlite3.connect(database)) as db:
            for key, value in [('current', {'revision': 1, 'settings': {'autoPlay': True, 'rememberPosition': True}}),
                               ('revision', 1)]:
                db.execute("INSERT OR REPLACE INTO state(owner,kind,key,value) VALUES (?,?,?,?)",
                           ('local-device', 'playback-startup-preferences-v1', key, json.dumps(value)))
            db.commit()
        before = read_playback(database)
        with (root / 'invalid.log').open('x', encoding='utf-8') as log:
            app = subprocess.Popen([str(args.binary.resolve()), f'--play={bad}'],
                                   env=env, stdout=log, stderr=log)
            try:
                wait_for('invalid-file ordinary GUI focused', lambda: active(app))
                focus_installed_control(access, app, '打开设置', (access.Role.PUSH_BUTTON, access.Role.TOGGLE_BUTTON))
                time.sleep(2.5)
                # WebKitGTK does not expose the footer's static live-region
                # message through AT-SPI here. Keep actual painted pixels for
                # review; state/output below remain machine-checked.
                screen = Gdk.get_default_root_window()
                image = Gdk.pixbuf_get_from_window(screen, 0, 0, screen.get_width(), screen.get_height())
                assert image is not None
                image.savev(str(root / 'invalid-notice.png'), 'png', [], [])
                assert app.poll() is None and not private_pipewire_stream(app.pid, env)
                assert read_playback(database) == before, 'invalid explicit open altered the restored queue or clock'
                silent = sink_tone(env)
                assert silent['rms'] < tone['rms'] * .01, 'invalid activation unexpectedly played audio'
            finally:
                stop(app)

    verify_proof_databases([root / 'state'])
    report = dict(ok=True, ordinaryProductionBinary=True, actualWebView=True,
                  acceptedNativeArgvAndUnicode=True, invalidFileSkipped=True, openedTwoInOrder=True,
                  firstFilePinnedDecoderAdvanced=True, secondFilePinnedDecoderAdvanced=True,
                  isolatedPipeWireNullSink=True, syntheticFlacPcm=tone, syntheticMp3Pcm=second_tone,
                  guiPauseHeld=True, runningProfileRejectsCompetingOrdinaryLaunch=True, invalidActivationScreenshot=True,
                  invalidActivationDidNotAutoplay=True, associationMetadataChanged=False,
                  successfulMusicServiceRequests=0, physicalOutputVerified=False, windowsNativeVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
