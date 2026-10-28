"""Production startup settings, real native decoding and five process lifetimes."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time

PHASES = {'initial': 6, 'autoplay-zero': 2, 'autoplay-resume': 4, 'repair': 4, 'repaired-reopen': 2}


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def audit(root):
    stages = {}
    for phase, count in PHASES.items():
        directory = root / 'run' / phase
        value = read(directory / 'complete.json')
        assert value['ok'] and value['phase'] == phase and value['stageCount'] == count
        assert value['realMusicRequests'] == 0 and value['decoder'] == 'libmpv' and value['audioOutput'] == 'null'
        files = list(directory.glob('ux_*.json'))
        assert len(files) == count
        stages[phase] = {p.stem[6:]: read(p) for p in files}
    initial = stages['initial']
    assert initial['startup_paused']['nativePlayer']['state'] == 'paused'
    assert abs(initial['startup_paused']['nativePlayer']['position'] - 12.5) < .2
    assert initial['options_default_light']['preferences']['settings'] == {'autoPlay': False, 'rememberPosition': True}
    assert initial['save_failed']['error'] and initial['save_failed']['autoPlay']
    assert initial['save_failed']['focused'] == '重新读取播放设置'
    assert not initial['read_recovered']['error']
    assert initial['save_failed']['preferences'] == initial['saved_start_zero']['preferences']
    for name in ['saved_start_zero', 'small_dark', 'save_failed', 'read_recovered']:
        assert initial[name]['nativePlayer']['state'] == 'paused'
        assert abs(initial[name]['nativePlayer']['position'] - 12.5) < .2
    zero = stages['autoplay-zero']['playing_zero']['nativePlayer']
    assert zero['state'] == 'playing' and .2 < zero['position'] < 8 and zero['error'] is None
    resume = stages['autoplay-resume']
    assert resume['playing_remembered']['nativePlayer']['state'] == 'playing'
    assert 17 <= resume['playing_remembered']['nativePlayer']['position'] < 25
    assert resume['reloaded_stopped']['nativePlayer']['state'] == 'idle'
    assert resume['reloaded_stopped']['preferences']['settings']['autoPlay']
    repaired = stages['repair']
    assert repaired['damaged_dark']['preferences']['damaged']
    assert repaired['repaired']['focused'] == '重新读取播放设置'
    for name in ['repaired', 'repaired_light']:
        assert repaired[name]['nativePlayer']['state'] == 'paused'
        assert repaired[name]['preferences']['settings'] == {'autoPlay': False, 'rememberPosition': True}
    final = read(root / 'run/repaired-reopen/complete.json')
    assert final['nativePlayer']['state'] == 'paused' and final['nativePlayer']['position'] >= 17
    assert final['preferences']['settings'] == {'autoPlay': False, 'rememberPosition': True}
    flat = [row for phase in stages.values() for row in phase.values()]
    assert {row['theme'] for row in flat} == {'light', 'dark'}
    assert any(row['width'] == 760 and row['height'] == 540 for row in flat)
    with sqlite3.connect((root / 'run/state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        row = db.execute("SELECT value FROM state WHERE owner='local-device' AND kind='playback-startup-preferences-v1' AND key='current'").fetchone()
        assert json.loads(row[0]) == {'revision': 5, 'settings': {'autoPlay': False, 'rememberPosition': True}}
        assert json.loads(db.execute("SELECT value FROM state WHERE owner='local-device' AND kind='playback-startup-preferences-v1' AND key='revision'").fetchone()[0]) == 5
        assert db.execute("SELECT count(*) FROM state WHERE owner='local-device' AND kind='playback-startup-recovery-v1'").fetchone()[0] == 1
        assert db.execute("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name='reject_startup'").fetchone()[0] == 0
    return {'ok': True, 'actualWebView': True, 'states': len(flat), 'processLifetimes': 5,
            'pageReloadDidNotRestartAudio': True, 'realMusicRequests': 0, 'physicalAudioOutputVerified': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true', help='local Linux GTK capture, not required in CI')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / directory
        path.mkdir(parents=True)
        env[variable] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if os.name != 'nt':
        # Keep GTK/AT-SPI runtime sockets out of another desktop probe's session.
        runtime = root / 'os-profile' / 'runtime'
        runtime.mkdir(mode=0o700)
        env['XDG_RUNTIME_DIR'] = str(runtime)
    if args.screenshots:
        import gi
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk
    captures = 0
    for phase, count in PHASES.items():
        env['CUMULUS_STARTUP_PHASE'] = phase
        output = root / 'run' / phase
        seen = set()
        with (root / (phase + '.log')).open('x', encoding='utf-8') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 150
                while process.poll() is None:
                    if time.monotonic() >= deadline:
                        raise TimeoutError(f'startup phase {phase}')
                    if args.screenshots:
                        for path in output.glob('ux_*.json'):
                            if path in seen:
                                continue
                            window = Gdk.get_default_root_window()
                            picture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                            assert picture is not None
                            picture.savev(str(path.with_suffix('.png')), 'png', [], [])
                            seen.add(path)
                    time.sleep(.1)
                failure = output / 'failed.json'
                assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
                assert process.returncode == 0, f'{phase}: exited {process.returncode}'
                if args.screenshots:
                    assert len(seen) == count, f'{phase}: missing screenshots'
                captures += len(seen)
            finally:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
    result = {**audit(root), 'screenshots': captures}
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
