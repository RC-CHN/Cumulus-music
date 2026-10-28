"""Actual WebView motion policy, failure recovery, native lyrics and restart.

The OS preference change and storage failure are explicitly injected by the
standalone probe. CSS/WAAPI/scroll calls, UI/IPC and libmpv remain real.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def launch(binary, env, log, output, capture):
    if not capture:
        return subprocess.run([str(binary)], env=env, stdout=log, stderr=log, timeout=180).returncode
    import gi
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gdk
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    seen = set()
    try:
        deadline = time.monotonic() + 180
        while process.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError('motion WebView did not finish')
            for path in output.glob('ux_*.json'):
                if path in seen:
                    continue
                window = Gdk.get_default_root_window()
                picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                assert picture is not None
                picture.savev(str(path.with_suffix('.png')), 'png', [], [])
                seen.add(path)
            time.sleep(.1)
        return process.returncode
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true', help='optional local Linux GTK captures')
    args = parser.parse_args()
    root = args.root.resolve()
    assert not root.exists(), 'new evidence directory required'
    root.mkdir(parents=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    states = 0
    for phase, count in [('first', 16), ('reopen', 2)]:
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            status = launch(args.binary.resolve(), env, log, output, args.screenshots)
        if (output / 'failed.json').exists():
            raise AssertionError((output / 'failed.json').read_text(encoding='utf-8'))
        assert status == 0, f'motion probe exit {status}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount'] == count and complete['realMusicRequests'] == 0
        assert complete['systemPreferenceEventInjected']
        assert len(complete['checks']) == (9 if phase == 'first' else 1)
        snapshots = list(output.glob('ux_*.json'))
        assert len(snapshots) == count
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == count
        for path in snapshots:
            value = json.loads(path.read_text(encoding='utf-8'))
            assert not value['overflow'] and value['theme'] in ['light', 'dark']
        if phase == 'first':
            assert all(complete[k] for k in ['lyricScrollVerified', 'lyricSmoothRestored', 'themeCancellationVerified', 'persistenceFailureVerified'])
            player = complete['nativePlayer']
            assert player['state'] == 'paused' and player['position'] > 0
            lyric = json.loads((output / 'ux_08_lyrics_instant.json').read_text(encoding='utf-8'))['nativePlayer']
            assert player['playId'] == lyric['playId'] and abs(player['position'] - lyric['position']) < .05
        else:
            assert complete['reopened']
        states += count
    result = {'ok': True, 'actualWebView': True, 'states': states,
              'screenshots': states if args.screenshots else 0, 'realMusicRequests': 0,
              'motionRestartVerified': True, 'systemPreferenceEventInjected': True,
              'physicalAudioOutputVerified': False}
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
