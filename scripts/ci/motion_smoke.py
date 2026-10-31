"""Actual WebView motion policy, failure recovery, native lyrics and restart.

The OS preference change and storage failure are explicitly injected by the
standalone probe. CSS/WAAPI/scroll calls, UI/IPC and libmpv remain real.
"""
import argparse
import json
import math
import os
import re
from pathlib import Path
import subprocess
import time


def paint_checks(picture, payload):
    """Check declared opaque padding/cover samples after CSS settles.

    This detects a stale compositor frame, not whole-dialog visual correctness.
    Other probes without dialog coordinates still use their own settled checks.
    """
    checks, samples = [], list(payload.get('paintSamples', []))
    ratio = payload.get('ratio', 1)
    for dialog in payload.get('dialogs', []):
        if not dialog['open'] or dialog['opacity'] != '1':
            continue
        rect = dialog['rect']
        samples.append({'x': rect['left'] + 4, 'y': rect['top'] + rect['height'] / 2,
                        'color': dialog['background']})
    for sample in samples:
        color = re.fullmatch(r'rgb\((\d+), (\d+), (\d+)\)', sample['color'])
        if color is None:
            continue
        # CSS coordinates name a point inside a physical pixel, not its
        # nearest integer boundary. At 125%, rounding a right-hand outline
        # sample can move it into the adjacent background pixel.
        x = math.floor((payload.get('screenX', 0) + sample['x']) * ratio)
        y = math.floor((payload.get('screenY', 0) + sample['y']) * ratio)
        expected = tuple(map(int, color.groups()))
        if not (0 <= x < picture.get_width() and 0 <= y < picture.get_height()):
            raise AssertionError('paint sample outside captured root window')
        offset = y * picture.get_rowstride() + x * picture.get_n_channels()
        actual = tuple(picture.get_pixels()[offset:offset + 3])
        checks.append({'point': [x, y], 'expected': expected, 'actual': actual,
                       'ok': actual == expected})
    return checks


def launch(binary, env, log, output, capture):
    if not capture:
        return subprocess.run([str(binary)], env=env, stdout=log, stderr=log, timeout=180).returncode
    env = {**env, 'CUMULUS_PROOF_CAPTURE_ACK': '1'}
    import gi
    gi.require_version('Gdk', '3.0')
    from gi.repository import Gdk
    process = subprocess.Popen([str(binary)], env=env, stdout=log, stderr=log)
    seen = set()
    try:
        deadline = time.monotonic() + 180
        while process.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError('WebView did not finish')
            for path in output.glob('ux_*.json'):
                if path in seen:
                    continue
                try:
                    payload = json.loads(path.read_text(encoding='utf-8'))
                except json.JSONDecodeError:
                    continue  # The native writer may still be writing this stage.
                window = Gdk.get_default_root_window()
                started = time.monotonic()
                while True:
                    picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                    assert picture is not None
                    checks = paint_checks(picture, payload)
                    if all(check['ok'] for check in checks) or time.monotonic() - started >= 3:
                        break
                    time.sleep(.08)
                picture.savev(str(path.with_suffix('.png')), 'png', [], [])
                if checks:
                    path.with_name('paint-' + path.name).write_text(json.dumps({
                        'waitSeconds': time.monotonic() - started, 'samples': checks,
                    }, indent=2) + '\n', encoding='utf-8')
                    assert all(check['ok'] for check in checks), 'dialog compositor frame remained stale'
                path.with_suffix('.captured').write_text('captured\n', encoding='utf-8')
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
    phases = [('first', 16), ('reopen', 2)]
    if os.name == 'nt':
        phases.append(('native-reduced', 2))
    for phase, count in phases:
        if os.name == 'nt':
            # Match the synthetic MQL baseline with Chromium's real CSS query.
            # This affects only the isolated WebView2 child, never OS settings
            # or the production app. Also test a real reduced-motion query.
            existing = re.sub(r'(?<!\S)--force-prefers-(?:no-)?reduced-motion(?=\s|$)', '',
                              env.get('WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS', '')).strip()
            flag = '--force-prefers-reduced-motion' if phase == 'native-reduced' else '--force-prefers-no-reduced-motion'
            env['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = (existing + ' ' + flag).strip()
        if phase == 'native-reduced':
            env['CUMULUS_MOTION_NATIVE_REDUCED'] = '1'
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
        assert complete['systemPreferenceEventInjected'] == (phase != 'native-reduced')
        assert len(complete['checks']) == (9 if phase == 'first' else 2 if phase == 'native-reduced' else 1)
        snapshots = list(output.glob('ux_*.json'))
        assert len(snapshots) == count
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == count
        for path in snapshots:
            value = json.loads(path.read_text(encoding='utf-8'))
            assert not value['overflow'] and value['theme'] in ['light', 'dark']
            assert value['nativeReduced'] == (phase == 'native-reduced')
        if phase == 'first':
            assert all(complete[k] for k in ['lyricScrollVerified', 'lyricSmoothRestored', 'themeCancellationVerified', 'persistenceFailureVerified'])
            player = complete['nativePlayer']
            assert player['state'] == 'paused' and player['position'] > 0
            lyric = json.loads((output / 'ux_08_lyrics_instant.json').read_text(encoding='utf-8'))['nativePlayer']
            assert player['playId'] == lyric['playId'] and abs(player['position'] - lyric['position']) < .05
        elif phase == 'reopen':
            assert complete['reopened']
        else:
            assert complete['nativeReduced'] and complete['nativePlayer']['state'] == 'paused'
            native = [json.loads(path.read_text(encoding='utf-8'))['nativePlayer'] for path in snapshots]
            assert len({player['playId'] for player in native}) == 1
            assert all(player['state'] == 'paused' for player in native)
        states += count
    result = {'ok': True, 'actualWebView': True, 'states': states,
              'screenshots': states if args.screenshots else 0, 'realMusicRequests': 0,
              'motionRestartVerified': True, 'systemPreferenceEventInjected': True,
              'nativeReducedMotionVerified': os.name == 'nt',
              'physicalAudioOutputVerified': False}
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
