"""One ordinary Linux production app, warm Unix relay, private actual PCM."""
import argparse
from contextlib import closing
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import socket
import subprocess

from diagnostics import verify_proof_databases
from installed_linux_audio import (
    Input, isolated_audio, private_pipewire_stream, read_playback, sink_tone, start_accessibility, stop,
    wait_for,
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
    state_dir = root / 'state'
    env['CUMULUS_DATA_DIR'] = str(state_dir)
    env['CUMULUS_DEVICE_IDENTITY'] = 'native'
    for key, suffix in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache')]:
        folder = root / suffix
        folder.mkdir(mode=0o700)
        env[key] = str(folder)
    start_accessibility()
    database = state_dir / 'core.sqlite3'
    binary = str(args.binary.resolve())

    def windows():
        value = subprocess.check_output(['xprop', '-root', '_NET_CLIENT_LIST'], text=True, timeout=3)
        result = {}
        for window in re.findall(r'0x[0-9a-f]+', value):
            response = subprocess.check_output(['xprop', '-id', window, '_NET_WM_PID'], text=True, timeout=3)
            if found := re.search(r'= (\d+)\s*$', response):
                result[window] = int(found[1])
        return result

    def active(app):
        for window, pid in windows().items():
            if pid != app.pid:
                continue
            value = subprocess.check_output(['xprop', '-root', '_NET_ACTIVE_WINDOW'], text=True, timeout=3)
            if value.strip().endswith(window):
                return window
        return None

    def activation(candidate_env, *options):
        return subprocess.run([binary, *options], env=candidate_env, capture_output=True, text=True, timeout=32)

    def playback():
        assert app.poll() is None, 'ordinary owner exited during handoff'
        return read_playback(database) if database.is_file() else {}

    def selected(path, elapsed):
        data = playback()
        queue = data.get('queue')
        position = data.get('position')
        if not queue or not position or position['state'] != 'playing' or position['seconds'] < elapsed:
            return None
        entry = next((row for row in queue['items'] if row['entry_id'] == queue['current']), None)
        if entry and position['entry_id'] == entry['entry_id'] and entry['resource']['native_open_file']['path'] == str(path):
            return data
        return None

    with isolated_audio(root, env) as seed:
        first = root / '1 合成 原曲.flac'
        second = root / '2 合成 转交.mp3'
        pcm_wave = root / '3 合成 转交.WAV'
        unsupported_wave = root / '4 合成 其他采样率.wav'
        bad = root / '0 无效.flac'
        first.write_bytes(seed.read_bytes())
        bad.write_text('https://example.invalid/never-open.flac\n', encoding='ascii')
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi',
                        '-i', 'sine=frequency=10000:sample_rate=48000:duration=45', str(second)],
                       check=True, timeout=20)
        for path, rate in [(pcm_wave, 44100), (unsupported_wave, 48000)]:
            subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-f', 'lavfi',
                            '-i', f'sine=frequency=440:sample_rate={rate}:duration=15',
                            '-ac', '2', '-c:a', 'pcm_s16le', str(path)], check=True, timeout=20)
        alias = root / 'same-canonical-state-root'
        alias.symlink_to(state_dir, target_is_directory=True)
        sender_env = dict(env, CUMULUS_DATA_DIR=str(alias))
        warm_env = dict(sender_env, CUMULUS_RENDERER='invalid-must-not-run-graphics')
        with (root / 'owner.log').open('x', encoding='utf-8') as log:
            app = subprocess.Popen([binary, '--play', str(first)], env=env, stdout=log, stderr=log)
            try:
                window = wait_for('ordinary owner GUI focused', lambda: active(app))
                before = wait_for('first cold-open native item is actually playing', lambda: selected(first, .6))
                wait_for('ordinary owner private PipeWire node', lambda: private_pipewire_stream(app.pid, env))
                old_tone = sink_tone(env)
                assert old_tone['amplitude440Hz'] > 200

                # Hold one real request after its READY marker and verify the
                # actual owner refuses a simultaneous one before path input.
                key = hashlib.sha256(b'cumulus-native-open-v1\0' + os.fsencode(state_dir)).hexdigest()
                address = f'\0io.cumulus.music.open.{os.geteuid()}.{key}'
                with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as held, \
                        socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as competing:
                    for stream in (held, competing):
                        stream.settimeout(2)
                        stream.connect(address)
                    assert held.recv(5, socket.MSG_WAITALL) == b'CFO1\x06'
                    assert competing.recv(5, socket.MSG_WAITALL) == b'CFO1\x05'
                    held.shutdown(socket.SHUT_WR)
                    assert held.recv(5, socket.MSG_WAITALL) == b'CFO1\x03'

                overridden = activation(dict(warm_env, CUMULUS_SESSION_DIR=str(root / 'unused-legacy-session')),
                                        f'--play={second}')
                assert overridden.returncode == 2
                assert playback()['queue'] == before['queue']
                assert not (root / 'unused-legacy-session').exists()
                rejected = activation(warm_env, f'--play={bad}')
                assert rejected.returncode == 3 and 'readable MP3, FLAC or supported PCM WAV' in rejected.stderr
                unchanged = wait_for('rejected warm open leaves original track playing', lambda: selected(first, 1.5))
                assert unchanged['queue'] == before['queue']

                with closing(Input(0, 0)) as inputs:
                    iconify = inputs.x11.XIconifyWindow
                    iconify.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int]
                    iconify.restype = ctypes.c_int
                    assert iconify(inputs.display, int(window, 16), 0), 'native minimize request failed'
                    inputs.x11.XFlush(inputs.display)
                wait_for('original window was actually minimized', lambda: active(app) != window)
                result = activation(warm_env, '--play', str(bad), str(second))
                assert result.returncode == 0, result.returncode
                after = wait_for('warm app picked its new native entry', lambda: selected(second, .6))
                wait_for('warm handoff reveals and focuses its original window', lambda: active(app) == window)
                assert list(windows().values()).count(app.pid) == 1
                assert len(windows()) == 1, 'competing native window after handoff'
                assert len(after['queue']['items']) == 1, 'invalid first item was not skipped'
                assert after['queue']['current'] != before['queue']['current']
                wait_for('same owner still routes to the private sink', lambda: private_pipewire_stream(app.pid, env))
                new_tone = sink_tone(env)
                assert new_tone['amplitude10000Hz'] > 200 and new_tone['amplitude440Hz'] < old_tone['amplitude440Hz'] * .1

                denied = activation(sender_env)
                assert denied.returncode == 2 and 'already running for this app profile' in denied.stderr
                assert playback()['queue'] == after['queue']

                unsupported = activation(warm_env, '--play', str(unsupported_wave))
                assert unsupported.returncode == 3
                assert playback()['queue'] == after['queue']
                result = activation(warm_env, '--play', str(pcm_wave))
                assert result.returncode == 0, result.returncode
                wave_after = wait_for('warm PCM WAV is actually playing', lambda: selected(pcm_wave, .6))
                assert wave_after['queue']['current'] != after['queue']['current']
                wait_for('same window is still focused', lambda: active(app) == window)
                assert list(windows().values()) == [app.pid]
                wait_for('held WAV is linked to the private monitor', lambda: private_pipewire_stream(app.pid, env))
                wave_tone = sink_tone(env)
                assert wave_tone['amplitude440Hz'] > 200 and wave_tone['amplitude10000Hz'] < new_tone['amplitude10000Hz'] * .1
            finally:
                stop(app)

    verify_proof_databases([state_dir])
    report = dict(ok=True, ordinaryProductionBinary=True, actualWebView=True,
                  sameCanonicalRootThroughSymlink=True, nativeUnicodeAndSpaces=True,
                  senderBypassesGraphicsBootstrap=True, competingRequestReceivedBusy=True,
                  legacySessionOverrideCannotForward=True,
                  invalidWarmSelectionPreservedExistingQueue=True, invalidFirstFileSkipped=True,
                  originalWindowRevealed=True, originalProcessOnly=True, onlyOnePlayingQueue=True,
                  ordinaryDuplicateDenied=True, beforePcm=old_tone, afterPcm=new_tone,
                  warmPcmWave=wave_tone, unsupportedWavePreservedExistingQueue=True,
                  successfulMusicServiceRequests=0, associationMetadataChanged=False,
                  physicalOutputVerified=False, windowsNativeVerified=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
