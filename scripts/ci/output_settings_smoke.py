"""Production output settings with synthetic devices and real native null decoding."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time

PHASES = {'initial': 13, 'reopen': 1}


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def audit(root):
    stages = {}
    for phase, count in PHASES.items():
        directory = root / 'run' / phase
        result = read(directory / 'complete.json')
        assert result['ok'] and result['stageCount'] == count
        assert result['realMusicRequests'] == 0 and result['deviceCatalog'] == 'synthetic'
        assert result['decoder'] == 'libmpv' and result['audioOutput'] == 'null'
        files = list(directory.glob('ux_*.json'))
        assert len(files) == count
        stages[phase] = {p.stem[6:]: read(p) for p in files}
    first = stages['initial']
    assert len({row['nativePlayer']['playId'] for row in first.values()}) == 1
    assert len({row['nativePlayer']['entryId'] for row in first.values()}) == 1
    for rows in stages.values():
        for row in rows.values():
            assert row['nativePlayer']['state'] == 'paused'
            assert abs(row['nativePlayer']['position'] - 12.5) < .2
    for name in ['missing_fallback', 'returned_sticky']:
        out = first[name]['outputSettings']
        assert out['selected']['id'] == 'fixture/a'
        assert out['engines'][0]['phase'] == 'fallback' and out['engines'][0]['configured'] == 'auto'
    assert first['save_failed']['error'] and first['save_failed']['selected'] == 'fixture/a'
    assert first['save_failed']['focused'] == '刷新音频输出设备'
    assert first['damaged']['outputSettings']['damaged']
    assert first['repaired']['outputSettings']['selected']['id'] == 'auto'
    assert first['raw_json_damaged']['outputSettings']['damaged']
    assert first['raw_json_repaired']['outputSettings']['selected']['id'] == 'auto'
    for name in ['repaired', 'raw_json_repaired']:
        assert '已应用' in first[name]['status']
        engine = first[name]['outputSettings']['engines'][0]
        assert engine['configured'] == 'auto' and engine['phase'] == 'configured'
        assert engine['revision'] == first[name]['outputSettings']['revision']
    final = stages['reopen']['reopened_b']['outputSettings']
    assert final['selected']['id'] == 'fixture/b' and final['engines'][0]['configured'] == 'fixture/b'
    assert final['revision'] == first['saved_b']['outputSettings']['revision']
    with sqlite3.connect((root / 'run/state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        backups = [json.loads(r[0]) for r in db.execute("SELECT value FROM state WHERE kind='audio-output-recovery-v1'")]
        assert len(backups) == 2 and {'current_raw': '{broken'} in backups
        assert db.execute("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name='reject_output'").fetchone()[0] == 0
    return {'ok': True, 'actualWebView': True, 'states': sum(PHASES.values()), 'processLifetimes': 2,
            'realMusicRequests': 0, 'deviceCatalog': 'synthetic', 'physicalEndpointVerified': False}


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
        env['CUMULUS_OUTPUT_PHASE'] = phase
        output = root / 'run' / phase
        seen = set()
        with (root / (phase + '.log')).open('x', encoding='utf-8') as log:
            process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 150
                while process.poll() is None:
                    if time.monotonic() >= deadline:
                        raise TimeoutError(f'output phase {phase}')
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
