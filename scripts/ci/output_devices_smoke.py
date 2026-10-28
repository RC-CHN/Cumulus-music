"""Linux native output routing on a private PipeWire/Pulse server, no hardware.

Requires PipeWire, pipewire-pulse, WirePlumber with the policy-only profile,
and pactl. Run the desktop mode under dbus-run-session / xvfb-run with a non-C
locale. This exercises the native engine after GUI startup, not a settings UI.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time


def wait_for(operation, predicate, label, timeout=10):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = operation()
        if predicate(value):
            return value
        time.sleep(0.05)
    raise AssertionError(f'timed out: {label}')


def run(binary, report, desktop, managed=False):
    report.mkdir(parents=True, exist_ok=False)
    children, logs = [], []
    reader = None
    with tempfile.TemporaryDirectory(prefix='cumulus-output-proof-') as temporary:
        root = Path(temporary)
        env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
        for key, folder in [('XDG_CONFIG_HOME', 'config'), ('XDG_DATA_HOME', 'data'),
                            ('XDG_STATE_HOME', 'state'), ('XDG_CACHE_HOME', 'cache')]:
            path = root / folder
            path.mkdir()
            env[key] = str(path)
        if managed:
            env['CUMULUS_MANAGED_OUTPUT_PROOF'] = '1'
        env.update(XDG_RUNTIME_DIR=str(root), PIPEWIRE_RUNTIME_DIR=str(root),
                   PIPEWIRE_REMOTE='pipewire-0', PIPEWIRE_CONFIG_DIR='/usr/share/pipewire',
                   PULSE_SERVER='unix:' + str(root / 'pulse/native'))

        def start(name, command, interactive=False):
            log = (report / (name + '.log')).open('x', encoding='utf-8')
            logs.append(log)
            child = subprocess.Popen(command, env=env, stdin=subprocess.PIPE if interactive else subprocess.DEVNULL,
                                     stdout=subprocess.PIPE if interactive else log, stderr=log,
                                     text=True, encoding='utf-8', bufsize=1)
            children.append(child)
            return child

        def pactl(*args, parse=False):
            result = subprocess.run(['pactl', *(['-f', 'json'] if parse else []), *args],
                                    env=env, capture_output=True, text=True, check=True, timeout=10)
            return json.loads(result.stdout) if parse else result.stdout.strip()

        try:
            start('pipewire', ['pipewire'])
            wait_for(lambda: (root / 'pipewire-0').exists(), bool, 'private PipeWire socket')
            # This profile contains policy only: no ALSA/Bluetooth/camera
            # monitors, so it cannot capture or activate host hardware nodes.
            start('wireplumber', ['wireplumber', '--profile', 'policy'])
            start('pulse', ['pipewire-pulse'])
            wait_for(lambda: (root / 'pulse/native').exists(), bool, 'private Pulse socket')

            def sink(letter):
                return pactl('load-module', 'module-null-sink', 'sink_name=cumulus-proof-' + letter,
                             'sink_properties=device.description=Cumulus-Proof-' + letter.upper())

            sink('a')
            module_b = sink('b')
            pactl('set-default-sink', 'cumulus-proof-a')
            sinks = pactl('list', 'sinks', parse=True)
            assert all(s['name'] in ['auto_null', 'cumulus-proof-a', 'cumulus-proof-b'] for s in sinks), 'hardware isolation'
            if desktop:
                env['CUMULUS_OUTPUT_DEVICE_PROOF'] = '1'
                env['CUMULUS_PROOF_REPORT'] = str(report / 'native.json')
                env['CUMULUS_PROOF_WEBVIEW_DIR'] = str(root / 'webview')
            child = start('native', [str(binary)] + ([] if desktop else ['--isolated-pulse']), True)
            replies = queue.Queue()
            transcript = (report / 'native-output.jsonl').open('x', encoding='utf-8')
            logs.append(transcript)

            def read():
                for line in child.stdout:
                    transcript.write(line)
                    transcript.flush()
                    try:
                        replies.put(json.loads(line))
                    except json.JSONDecodeError:
                        pass  # Native platform logging is retained, not a reply.
                replies.put({'processClosed': True})

            reader = threading.Thread(target=read, daemon=True)
            reader.start()
            assert replies.get(timeout=20).get('ready'), 'native engine ready'

            def request(kind, ok=True, **fields):
                child.stdin.write(json.dumps({'kind': kind, **fields}) + '\n')
                child.stdin.flush()
                value = replies.get(timeout=10)
                assert value.get('ok') is ok, (kind, value)
                return value.get('value')

            def devices():
                return request('devices')

            def state():
                return request('state')

            def names(value):
                return [d['id'] for d in value['devices']]

            def select(letter):
                request('select', id='pulse/cumulus-proof-' + letter)

            def routed(letter, paused=False):
                def observe():
                    targets = {s['index']: s['name'] for s in pactl('list', 'sinks', parse=True)}
                    streams = pactl('list', 'sink-inputs', parse=True)
                    assert len(streams) <= 1, 'one native PCM stream'
                    return (targets.get(streams[0]['sink']), streams[0]['corked']) if streams else None
                return wait_for(observe, lambda s: s == ('cumulus-proof-' + letter, paused), 'PCM route ' + letter)

            initial = wait_for(devices, lambda d: all('pulse/cumulus-proof-' + s in names(d) for s in ['a', 'b']), 'devices discovered')
            assert initial['configured'] == 'auto' and initial['active_driver'] is None
            for invalid in ['', 'bad\0device', 'x' * 4097, 'pulse/cumulus-proof-missing']:
                request('select', ok=False, id=invalid)
            assert devices()['configured'] == 'auto'
            select('a')
            entry = request('load')['entry']
            assert state()['paused']
            request('pause', paused=False)
            wait_for(state, lambda s: s['position'] is not None and s['position'] > 0.15, 'decoded PCM clock')
            routed('a')
            select('b')
            routed('b')
            assert state()['entry'] == entry and not state()['paused']
            request('pause', paused=True)
            select('a')
            routed('a', paused=True)
            after = state()
            assert after['paused'] and after['entry'] == entry
            time.sleep(0.2)
            settled = state()
            assert settled['paused']
            time.sleep(0.2)
            assert abs(state()['position'] - settled['position']) < 0.05
            request('pause', paused=False)
            routed('a')
            pactl('unload-module', module_b)
            wait_for(devices, lambda d: 'pulse/cumulus-proof-b' not in names(d), 'inactive device removed')
            request('select', ok=False, id='pulse/cumulus-proof-b')
            assert devices()['configured'] == 'pulse/cumulus-proof-a'
            module_b = sink('b')
            wait_for(devices, lambda d: 'pulse/cumulus-proof-b' in names(d), 'device returned')
            select('b')
            routed('b')
            pactl('unload-module', module_b)
            removed = wait_for(devices, lambda d: 'pulse/cumulus-proof-b' not in names(d), 'active device removed')
            if managed:
                wait_for(devices, lambda d: d['configured'] == 'auto', 'automatic fallback after removal')
                routed('a')
                fallback = state()
                assert fallback['preference']['selection']['id'] == 'pulse/cumulus-proof-b'
                assert fallback['entry'] == entry and not fallback['paused']
            # Also verify an explicit choice after removal.
            select('a')
            routed('a')
            restored = state()
            assert restored['entry'] == entry and not restored['paused'] and restored['driver'] == 'pulse'
            wait_for(state, lambda s: s['position'] is not None and s['position'] > restored['position'] + 0.1, 'recovered clock')
            assert all(p.poll() is None for p in children), 'all native services remained alive'
            request('quit')
            assert child.wait(timeout=10) == 0
            reader.join(timeout=2)
            if desktop:
                native = json.loads((report / 'native.json').read_text(encoding='utf-8'))
                assert native['productionDesktopStartup'] and native['numericLocaleCompatible']
            proof = {'ok': True, 'desktop': desktop, 'syntheticSilence': True, 'privateVirtualSinks': 2,
                     'initialEnumeration': True, 'invalidSelectionRejected': 4,
                     'pcmRoutesObserved': ['a', 'b', 'a', 'b', 'a'], 'pausePreserved': True,
                     'pulseCorkAndPausedClockVerified': True,
                     'sameEntryAcrossSwitches': True, 'removedDeviceRejected': True,
                     'reappearanceEnumerated': True, 'explicitRecoveryAfterRemoval': True,
                     'configuredAfterActiveRemoval': removed['configured'],
                     'managedPolicyVerified': managed, 'automaticFallbackVerified': managed,
                     'physicalSpeakersVerified': False, 'settingsUiVerified': False, 'remoteRequests': 0}
            (report / 'report.json').write_text(json.dumps(proof, indent=2) + '\n', encoding='utf-8')
            print(json.dumps(proof))
        finally:
            for child in reversed(children):
                if child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=3)
            if reader is not None:
                reader.join(timeout=2)
            for log in logs:
                log.close()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    parser.add_argument('--desktop', action='store_true')
    parser.add_argument('--managed', action='store_true')
    arguments = parser.parse_args()
    run(arguments.binary.resolve(), arguments.root.resolve(), arguments.desktop, arguments.managed)
