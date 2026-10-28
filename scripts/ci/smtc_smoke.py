"""Real Windows SMTC broker plus a separate session consumer, synthetic PCM only.

No real accounts; no controls sent to unrelated sessions. Consumer delivery is
not business success: every action is checked against the host's actual core.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time
from package import windows_media


def main():
    assert sys.platform == 'win32', 'SMTC runtime proof requires Windows'
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    native = root / 'runtime'
    native.mkdir()
    windows_media(native, Path(os.environ['CUMULUS_MSYS2_ROOT']))
    executable = native / 'smtc_probe.exe'
    shutil.copy2(args.binary, executable)
    run = root / 'run'
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    system = Path(os.environ['SystemRoot'])
    env['PATH'] = os.pathsep.join(map(str, [native, system / 'System32', system]))
    env['CUMULUS_PROOF_DIR'] = str(run)
    for key in ['APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP']:
        directory = root / key.lower()
        directory.mkdir()
        env[key] = str(directory)
    process = None
    steps = []
    deadline = time.monotonic() + 240
    sequence = 0
    def wait(condition, label):
        until = min(time.monotonic() + 15, deadline)
        while time.monotonic() < until:
            result = condition()
            if result:
                return result
            assert process.poll() is None, f'host exited {process.returncode}: {label}'
            time.sleep(.05)
        raise AssertionError('timed out: ' + label)
    def state():
        return json.loads((run / 'state.json').read_text(encoding='utf-8'))
    def fixture(kind):
        nonlocal sequence
        sequence += 1
        pending = run / 'command.pending'
        pending.write_text(json.dumps({'kind': kind, 'id': sequence}), encoding='utf-8')
        pending.replace(run / 'command.json')
        reply = run / f'ack-{sequence}.json'
        wait(reply.exists, 'fixture ' + kind)
        assert json.loads(reply.read_text())['ok'], kind
    def consumer(command='snapshot'):
        assert time.monotonic() < deadline, 'overall SMTC proof deadline'
        result = subprocess.run([str(executable), '--consumer', prefix, command],
                                env=env, capture_output=True, encoding='utf-8',
                                timeout=min(20, deadline - time.monotonic()))
        with (root / 'consumer.log').open('a', encoding='utf-8') as log:
            log.write(json.dumps({'command': command, 'exit': result.returncode,
                                  'stdout': result.stdout, 'stderr': result.stderr}) + '\n')
        assert result.returncode == 0, f'consumer {command} exited {result.returncode}'
        return json.loads(result.stdout)
    def action(command):
        value = consumer(command)
        assert value.get('found') and value.get('delivered'), (command, value)
    def os_state(**expected):
        value = consumer()
        return value if value.get('found') and all(value.get(k) == v for k, v in expected.items()) else None
    try:
        with (root / 'host.log').open('x', encoding='utf-8') as log:
            process = subprocess.Popen([str(executable)], env=env, stdout=log, stderr=log)
            prefix = f'CumulusSMTCProof-{process.pid}: '
            wait(lambda: (run / 'ready.json').exists() and (run / 'state.json').exists(), 'native GUI')
            assert state()['engines'] == 0 and not consumer()['found']
            steps.append('lazy-unselected')
            fixture('start')
            wait(lambda: state()['status'] == 'Playing' and state()['position'] > .15, 'PCM clock')
            metadata = wait(lambda: os_state(status=4), 'Windows broker session')
            assert metadata['title'] == prefix + '合成歌曲 2' and metadata['artist'] == '测试歌手'
            assert metadata['end'] == 300_000_000 and metadata['min'] == 0
            assert metadata['canPlay'] and metadata['canPause'] and metadata['canNext']
            first = state()['id']
            steps.append('actual-broker-metadata')
            action('pause')
            wait(lambda: state()['status'] == 'Paused', 'core pause')
            wait(lambda: os_state(status=5), 'OS pause')
            paused = state()['position']
            time.sleep(.4)
            assert abs(state()['position'] - paused) < .04
            accepted_seeks = len(state()['nativeSeeks'])
            action('seek')
            wait(lambda:len(state()['nativeSeeks'])>accepted_seeks, 'accepted native seek')
            trace = state()['nativeSeeks']
            assert len(trace)==accepted_seeks+1 and trace[-1]==4, trace
            action('play')
            wait(lambda: state()['status'] == 'Playing' and state()['position'] > 4.1, 'native output after seek')
            assert state()['id'] == first
            steps.append('pause-seek-resume')
            action('next')
            wait(lambda: state()['id'] != first and state()['status'] == 'Playing', 'next native song')
            wait(lambda: os_state(title=prefix + '合成歌曲 3'), 'next metadata')
            action('previous')
            wait(lambda: state()['id'] == first and state()['status'] == 'Playing', 'previous native song')
            wait(lambda: os_state(title=prefix + '合成歌曲 2'), 'previous metadata')
            action('pause')
            wait(lambda: state()['status'] == 'Paused', 'preference pause')
            steps.append('next-previous')
            action('rate')
            wait(lambda: state()['speed'] == 1.5, 'native rate')
            wait(lambda: os_state(rate=1.5), 'published rate')
            action('shuffle')
            wait(lambda: state()['mode'] == 'random', 'native random mode')
            wait(lambda: os_state(shuffle=True), 'published shuffle')
            action('repeat')
            wait(lambda: state()['mode'] == 'single_loop', 'native repeat mode')
            wait(lambda: os_state(shuffle=False, repeat=1), 'published repeat')
            steps.append('rate-shuffle-repeat')
            fixture('reject')
            marker = 'Windows media command failed or its selection changed'
            failures = (root / 'host.log').read_text(encoding='utf-8').count(marker)
            action('normal-rate')
            wait(lambda: (root / 'host.log').read_text(encoding='utf-8').count(marker) > failures, 'persistence failure actually handled')
            assert state()['speed'] == 1.5
            wait(lambda: os_state(rate=1.5), 'OS actual rate after rollback')
            fixture('allow')
            action('normal-rate')
            wait(lambda: state()['speed'] == 1, 'explicit rate recovery')
            steps.append('save-failure-rollback-and-explicit-recovery')
            fixture('hide')
            action('play')
            wait(lambda: state()['status'] == 'Playing', 'hidden play')
            before = state()
            wait(lambda: state()['position'] > before['position'] + .3, 'hidden PCM progress')
            assert not state()['visible'] and state()['engines'] == before['engines']
            action('stop')
            wait(lambda: state()['status'] == 'Stopped', 'native stop')
            wait(lambda: os_state(status=3), 'published stop')
            steps.append('hidden-playback-stop')
            fixture('switch')
            wait(lambda: not consumer()['found'], 'account retirement removes metadata')
            fixture('start')
            wait(lambda: os_state(status=4), 'new account session')
            assert state()['id'] != first
            fixture('clear')
            wait(lambda: not consumer()['found'], 'clear removes metadata')
            steps.append('account-retirement-and-clear')
            fixture('exit')
            assert process.wait(timeout=10) == 0
            process = None
        report = {'ok': True, 'steps': steps, 'actualTauriWindow': True,
                  'actualWindowsSmtcBroker': True, 'separateConsumerProcess': True,
                  'syntheticPcmLibmpvNull': True, 'realMusicRequests': 0,
                  'nativeSeekAccepted': 4, 'preciseDecodedSeekVerified': False,
                  'physicalAudioOutputVerified': False, 'physicalShellPanelVerified': False}
        (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        print(json.dumps(report))
    finally:
        (root / 'completed-steps.json').write_text(json.dumps(steps), encoding='utf-8')
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)


if __name__ == '__main__':
    main()
