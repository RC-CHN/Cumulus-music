"""Verify cold and warm native file activation in the exact Windows portable app."""
import argparse
from array import array
from contextlib import closing, contextmanager
import ctypes
from ctypes import wintypes
import hashlib
import json
import math
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import uuid
import wave

from diagnostics import verify_proof_databases
from windows_native_open_retained import ARTIFACT_ID, BINARY_SHA256, REPOSITORY, verified_producer
from windows_system_master_volume_runtime import collect_sample
from windows_webview_inputs import ci_identity
from windows_zip_smoke import portable_runtime


def require_hosted_windows(env):
    assert sys.platform == 'win32' and sys.maxsize > 2**32, 'native Windows x64 Python is required'
    assert tuple(env.get(key) for key in (
        'GITHUB_ACTIONS', 'RUNNER_ENVIRONMENT', 'RUNNER_OS', 'RUNNER_ARCH',
    )) == ('true', 'github-hosted', 'Windows', 'X64'), 'use a disposable GitHub-hosted Windows x64 runner'
    return ci_identity(env)


NULL_OUTPUT_MARKER = 'Cumulus native-open proof audio: null'


def scratch_environment(root, system, app_dir, audio_output):
    env = {name: os.environ[name] for name in (
        'COMSPEC', 'ProgramData', 'ProgramFiles', 'ProgramFiles(x86)', 'ProgramW6432',
        'NUMBER_OF_PROCESSORS', 'PROCESSOR_ARCHITECTURE',
    ) if name in os.environ}
    env.update(SystemRoot=str(system), WINDIR=str(system),
               PATH=os.pathsep.join(map(str, (app_dir, system / 'System32', system))),
               CUMULUS_DATA_DIR=str(root / 'state'), CUMULUS_DEVICE_IDENTITY='native',
               PYTHONUTF8='1')
    for name, leaf in (
        ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'), ('USERPROFILE', 'user'),
        ('TEMP', 'temp'), ('TMP', 'temp'), ('WEBVIEW2_USER_DATA_FOLDER', 'webview'),
    ):
        folder = root / leaf
        folder.mkdir(exist_ok=True)
        env[name] = str(folder)
    env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = (
        '--disable-background-networking --host-resolver-rules="MAP * ~NOTFOUND, EXCLUDE localhost"'
    )
    if audio_output == 'null':
        env['CUMULUS_WINDOWS_NATIVE_OPEN_PROOF_OUTPUT'] = 'null'
    return env


@contextmanager
def block_app_network(binary, system):
    # The unique rule can only affect the executable in this proof's temporary
    # extraction, including its short-lived forwarding/graphics processes.
    rule = 'CumulusNativeOpenProof-' + uuid.uuid4().hex
    env = dict(os.environ, CUMULUS_PROOF_RULE=rule, CUMULUS_PROOF_BINARY=str(binary))
    powershell = system / 'System32/WindowsPowerShell/v1.0/powershell.exe'

    def run(script):
        subprocess.run(
            [powershell, '-NoLogo', '-NoProfile', '-NonInteractive', '-Command',
             "$ErrorActionPreference='Stop';\n" + script],
            env=env, check=True, timeout=35,
        )

    try:
        run("""
$profiles = @(Get-NetFirewallProfile -PolicyStore ActiveStore)
if ($profiles.Count -ne 3 -or @($profiles | Where-Object Enabled -ne 'True').Count -ne 0 -or
    (Get-Service -Name MpsSvc).Status -ne 'Running') { throw 'Firewall is not active' }
New-NetFirewallRule -Name $env:CUMULUS_PROOF_RULE -DisplayName $env:CUMULUS_PROOF_RULE `
  -Direction Outbound -Action Block -Program $env:CUMULUS_PROOF_BINARY -Profile Any `
  -Enabled True -PolicyStore PersistentStore | Out-Null
$active = Get-NetFirewallRule -Name $env:CUMULUS_PROOF_RULE -PolicyStore ActiveStore
$filter = $active | Get-NetFirewallApplicationFilter
if ($active.Direction -ne 'Outbound' -or $active.Action -ne 'Block' -or
    $active.Enabled -ne 'True' -or $filter.Program -ne $env:CUMULUS_PROOF_BINARY) {
  throw 'The exact outbound rule is not active'
}
""")
        yield
    finally:
        run("""
if (Get-NetFirewallRule -Name $env:CUMULUS_PROOF_RULE -PolicyStore PersistentStore -ErrorAction SilentlyContinue) {
  Remove-NetFirewallRule -Name $env:CUMULUS_PROOF_RULE -PolicyStore PersistentStore
}
""")


def create_wave(path, rate, frequency, seconds=24):
    # A whole number of tone periods in one second; little-endian stereo PCM.
    mono = (round(2048 * math.sin(math.tau * frequency * i / rate)) for i in range(rate))
    pcm = array('h', (sample for value in mono for sample in (value, value)))
    if sys.byteorder != 'little':
        pcm.byteswap()
    with wave.open(str(path), 'wb') as output:
        output.setparams((2, 2, rate, 0, 'NONE', 'not compressed'))
        output.writeframes(pcm.tobytes() * seconds)


def media_fixtures(root, encoder, env):
    cold = root / '冷启动 合成.WAV'
    warm = root / '热转交 合成.flac'
    mp3 = root / '热转交 合成.mp3'
    source = root / '热转交 输入.wav'
    invalid = root / '0 无效音频.flac'
    create_wave(cold, 44100, 440)
    create_wave(source, 48000, 880)
    invalid.write_text('https://example.invalid/not-a-media-file\n', encoding='ascii')
    for output, codec, extra, title in (
        (warm, 'flac', (), 'Warm retained FLAC'),
        (mp3, 'libmp3lame', ('-b:a', '192k', '-id3v2_version', '3'), 'Warm retained MP3'),
    ):
        subprocess.run(
            [encoder, '-hide_banner', '-v', 'error', '-nostdin', '-protocol_whitelist', 'file',
             '-f', 'wav', '-i', source, '-c:a', codec, *extra, '-metadata', f'title={title}', output],
            cwd=root, env={**env, 'PATH': str(encoder.parent) + os.pathsep + env['PATH']},
            check=True, timeout=30,
        )
    return cold, warm, mp3, invalid


class NativeWindows:
    def __init__(self):
        self.user = ctypes.WinDLL('user32', use_last_error=True)
        self.enum_fn = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
        for name, args, result in (
            ('EnumWindows', [self.enum_fn, wintypes.LPARAM], wintypes.BOOL),
            ('GetWindowThreadProcessId', [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)], wintypes.DWORD),
            ('GetWindowTextW', [wintypes.HWND, wintypes.LPWSTR, ctypes.c_int], ctypes.c_int),
            ('IsIconic', [wintypes.HWND], wintypes.BOOL),
            ('IsWindowVisible', [wintypes.HWND], wintypes.BOOL),
            ('ShowWindowAsync', [wintypes.HWND, ctypes.c_int], wintypes.BOOL),
            ('GetForegroundWindow', [], wintypes.HWND),
        ):
            function = getattr(self.user, name)
            function.argtypes, function.restype = args, result

    def app_windows(self, owner):
        windows = []

        @self.enum_fn
        def collect(window, _):
            process = wintypes.DWORD()
            title = ctypes.create_unicode_buffer(64)
            self.user.GetWindowThreadProcessId(window, ctypes.byref(process))
            if process.value == owner and self.user.GetWindowTextW(window, title, len(title)) and title.value == 'Cumulus':
                windows.append(window)
            return True

        assert self.user.EnumWindows(collect, 0), 'EnumWindows failed'
        return windows

    def minimize(self, window):
        assert self.user.ShowWindowAsync(window, 6), 'native minimize failed'  # SW_MINIMIZE

    def visible_restored(self, window):
        return self.user.IsWindowVisible(window) and not self.user.IsIconic(window)


def wait_for(description, predicate, timeout=25):
    stop = time.monotonic() + timeout
    while time.monotonic() < stop:
        if value := predicate():
            return value
        time.sleep(.08)
    raise AssertionError(description)


def read_playback(database):
    if not database.is_file():
        return {}
    try:
        with closing(sqlite3.connect(database.as_uri() + '?mode=ro', uri=True, timeout=.3)) as db:
            rows = db.execute(
                "SELECT key,value FROM state WHERE owner='local-guest' AND kind='playback' "
                "AND key IN ('queue','position')"
            ).fetchall()
    except sqlite3.OperationalError as error:
        if error.sqlite_errorcode in {sqlite3.SQLITE_BUSY, sqlite3.SQLITE_LOCKED} or str(error) == 'no such table: state':
            return {}
        raise
    return {key: json.loads(value) for key, value in rows}


def playing(snapshot, title, seconds):
    queue, position = snapshot.get('queue'), snapshot.get('position')
    if not queue or not position or position['state'] != 'playing' or position['seconds'] < seconds:
        return None
    if queue['current'] != position['entry_id']:
        return None
    entry = next(row for row in queue['items'] if row['entry_id'] == queue['current'])
    if entry['resource']['metadata']['title'] != title:
        return None
    return queue, position


def held_entry(queue, *, rate, format_name):
    entry, = queue['items']
    assert queue['current'] == entry['entry_id']
    assert entry['source'] == {'kind': 'local', 'nativeOpen': True}
    resource = entry['resource']
    held = resource['native_open_file']
    assert set(held) == {'held_id'} and str(uuid.UUID(held['held_id'])) == held['held_id'], 'expected a live handle receipt'
    metadata = resource['metadata']
    assert (metadata['format'], metadata['sample_rate'], metadata['channels']) == (format_name, rate, 2)
    return entry['entry_id']


def verify_app(root, evidence, binary, encoder, env, audio_output):
    cold, warm, mp3, invalid = media_fixtures(root, encoder, env)
    windows = NativeWindows()
    database = Path(env['CUMULUS_DATA_DIR']) / 'core.sqlite3'
    with (evidence / 'owner.log').open('xb') as log, (evidence / 'sender.log').open('x', encoding='utf-8') as send_log:
        webview_data = Path(env['WEBVIEW2_USER_DATA_FOLDER'])
        subprocess.run(
            [binary, '--cumulus-renderer-diagnostics'], cwd=binary.parent, env=env,
            stdout=log, stderr=log, check=True, timeout=50,
        )
        assert not any(webview_data.iterdir()), 'renderer probe wrote into the app WebView2 profile'
        app = subprocess.Popen([binary, '--play', cold], cwd=binary.parent, env=env, stdout=log, stderr=log)
        last_observation = {}
        window = None
        try:
            def playback():
                assert app.poll() is None, 'the owner exited before the handoff finished'
                snapshot = read_playback(database)
                if snapshot:
                    queue = snapshot.get('queue', {})
                    position = snapshot.get('position', {})
                    selected = next((entry for entry in queue.get('items', [])
                                     if entry['entry_id'] == queue.get('current')), {})
                    last_observation.update(
                        current=queue.get('current'), state=position.get('state'),
                        seconds=position.get('seconds'),
                        codec=selected.get('resource', {}).get('metadata', {}).get('format'),
                    )
                return snapshot

            def activate(label, expected, *args, **overrides):
                sender = subprocess.run(
                    [binary, *args], cwd=binary.parent, env={**env, **overrides},
                    capture_output=True, encoding='utf-8', errors='replace', timeout=28,
                )
                print(f'{label}: {sender.returncode}\n{sender.stderr}', file=send_log, flush=True)
                assert sender.returncode == expected, f'{label}: unexpected exit {sender.returncode}'

            window, = wait_for('no ordinary Cumulus window', lambda: windows.app_windows(app.pid))
            if audio_output == 'null':
                wait_for('exact package did not opt into the proof-only audio sink', lambda:
                         NULL_OUTPUT_MARKER.encode('ascii') in (evidence / 'owner.log').read_bytes())
            cold_queue, cold_position = wait_for(
                'cold WAV clock did not advance', lambda: playing(playback(), cold.stem, .6),
            )
            cold_entry = held_entry(cold_queue, rate=44100, format_name='pcm_s16le')
            no_graphics = {'CUMULUS_RENDERER': 'invalid-sender-must-not-probe-graphics'}
            unused_session = root / 'unused-session'
            activate('legacy session override', 2, '--play', warm,
                     CUMULUS_SESSION_DIR=str(unused_session), **no_graphics)
            assert not unused_session.exists() and playback()['queue'] == cold_queue
            activate('invalid held selection', 4, '--play', invalid, **no_graphics)
            assert playback()['queue'] == cold_queue, 'invalid open changed the original queue'
            windows.minimize(window)
            wait_for('original window was not minimized', lambda: windows.user.IsIconic(window))
            activate('warm FLAC', 0, '--play', invalid, warm, **no_graphics)
            warm_queue, warm_position = wait_for(
                'warm FLAC clock did not advance', lambda: playing(playback(), 'Warm retained FLAC', .6),
            )
            warm_entry = held_entry(warm_queue, rate=48000, format_name='flac')
            assert warm_entry != cold_entry
            wait_for('handoff did not restore the owner window', lambda: windows.visible_restored(window))
            assert windows.app_windows(app.pid) == [window], 'handoff did not retain the original HWND'
            activate('ordinary duplicate', 2)
            assert playback()['queue'] == warm_queue, 'duplicate launch changed the live queue'
            wait_for('warm decoder did not continue on the retained row', lambda: playing(
                playback(), 'Warm retained FLAC', warm_position['seconds'] + .5,
            ))
            activate('warm MP3', 0, '--play', mp3, **no_graphics)
            mp3_queue, mp3_position = wait_for(
                'warm MP3 clock did not advance', lambda: playing(playback(), 'Warm retained MP3', .6),
            )
            assert held_entry(mp3_queue, rate=48000, format_name='mp3') not in {cold_entry, warm_entry}
            assert windows.app_windows(app.pid) == [window]
            return dict(
                originalWindowRestored=True, foregroundObserved=windows.user.GetForegroundWindow() == window,
                sameOwnerWindowObserved=True, ownerPid=app.pid, ownerHwnd=window,
                rendererProbeProfileIsolated=True, coldWavClockSeconds=cold_position['seconds'],
                warmFlacClockSeconds=warm_position['seconds'], warmMp3ClockSeconds=mp3_position['seconds'],
                nativeRetainedReceipts=True,
                invalidPreservedQueue=True, invalidFirstWarmFileSkipped=True,
                legacySessionOverrideDenied=True, ordinaryDuplicateDenied=True, senderBypassedGraphics=True,
            )
        except Exception:
            print(json.dumps(dict(lastPlayback=last_observation, ownerPid=app.pid,
                                  ownerExit=app.poll(), ownerHwnd=window)), file=send_log, flush=True)
            raise
        finally:
            if app.poll() is None:
                app.terminate()
            app.wait(timeout=15)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ('packages', 'root', 'fixture-encoder', 'endpoint-probe'):
        parser.add_argument('--' + name, required=True, type=Path)
    parser.add_argument('--audio-output', choices=('default', 'null'), default='default')
    parser.add_argument('--retained-null', action='store_true',
                        help='Use only the fixed, earlier diagnostic package; never a release.')
    args = parser.parse_args()
    runner = require_hosted_windows(os.environ)
    identity = runner
    if args.retained_null:
        assert args.audio_output == 'null' and os.environ['GITHUB_EVENT_NAME'] == 'workflow_dispatch'
        assert os.environ['GITHUB_REPOSITORY'] == REPOSITORY
        assert os.environ['GITHUB_REF'] == 'refs/heads/validation/phase203-retained-windows-native-open'
        identity = verified_producer()
    system = Path(os.environ['SystemRoot']).resolve(strict=True)
    encoder = args.fixture_encoder.resolve(strict=True)
    assert encoder.is_file() and encoder.name.casefold() == 'ffmpeg.exe'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    # CI's generic failure collector automatically archives this literal
    # evidence root. Keep mutable SQLite and WebView profiles outside it.
    with tempfile.TemporaryDirectory(prefix='native-open-', ignore_cleanup_errors=True) as directory:
        scratch = Path(directory) / 'profile'
        scratch.mkdir()
        portable = portable_runtime(
            args.packages.resolve(strict=True), identity['commit'], Path(directory),
            ci_run=identity['ciRun'], ci_attempt=identity['ciAttempt'], require_package_flac=True,
        )
        binary = (portable / 'cumulus.exe').resolve(strict=True)
        with binary.open('rb') as source:
            binary_sha = hashlib.file_digest(source, 'sha256').hexdigest()
        if args.retained_null:
            assert binary_sha == BINARY_SHA256, 'retained portable executable digest mismatch'
        report = dict(ok=False, **identity, binarySha256=binary_sha,
                      proofAudioOutput=args.audio_output,
                      ordinaryWindowsPortableApp=args.audio_output == 'default',
                      appOutboundBlocked=False, defaultOutputClockVerified=False,
                      physicalAudioOutputVerified=False)
        if args.retained_null:
            report.update(retainedDiagnosticArtifact=ARTIFACT_ID, diagnosticRunner=runner)
        try:
            report['failureStage'] = 'default_render_endpoint_probe'
            endpoint = collect_sample(args.endpoint_probe.resolve(strict=True))
            report['defaultRenderEndpoint'] = endpoint
            if args.audio_output == 'default':
                assert endpoint['kind'] == 'endpoint_readable', (
                    f"default-render endpoint is not verified: {endpoint['kind']}"
                )
            report['failureStage'] = 'application_firewall'
            with block_app_network(binary, system):
                report['appOutboundBlocked'] = True
                report['failureStage'] = 'native_file_activation'
                result = verify_app(
                    scratch, root, binary, encoder,
                    scratch_environment(scratch, system, portable, args.audio_output),
                    args.audio_output,
                )
            report['failureStage'] = 'proof_data_audit'
            verify_proof_databases([scratch / 'state'])
            report.update(result, ok=True, failureStage=None,
                          defaultOutputClockVerified=args.audio_output == 'default')
        finally:
            (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
