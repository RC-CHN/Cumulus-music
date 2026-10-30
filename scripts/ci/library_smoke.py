"""Grouped library layouts, pagination, playback, order and read recovery via production IPC and an isolated in-memory backend."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_LIBRARY_UI_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/library_ui_probe.js'
    script = root / 'library.js'
    script.write_text(source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    results = {}
    for phase, count in [('first', 40), ('reopen', 2)]:
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, root / phase, args.screenshots)
        failed = root / phase / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'probe exited {code}'
        results[phase] = verify(root / phase, count, args.screenshots)
    first = results['first']
    assert first['orderVerified'] and first['playbackVerified'] and first['returnVerified']
    assert first['syntheticLibrary']['writes'] == 1
    assert results['reopen']['reopened']
    playing = json.loads((root / 'first/ux_29_playing.json').read_text())['nativePlayer']
    paused = json.loads((root / 'first/ux_30_paused.json').read_text())['nativePlayer']
    assert playing['state'] == 'playing' and paused['state'] == 'paused'
    assert playing['origin'] == paused['origin'] == {'kind': 'playlist', 'id': '2'}
    assert playing['queueLen'] == paused['queueLen'] == 40 and paused['position'] > 0
    assert playing['playId'] == paused['playId']
    result = {'ok': True, 'actualWebView': True, 'stageCount': 42,
              'screenshots': 42 if args.screenshots else 0, 'restartVerified': True,
              'physicalDisplayVerified': False, 'windowsVisualVerified': False,
              'realMusicRequests': 0, 'realAccountWrites': 0}
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


def verify(output, count, capture):
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['libraryUi'] and complete['stageCount'] == count
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert len(list(output.glob('ux_*.json'))) == count
    if capture:
        assert len(list(output.glob('ux_*.png'))) == count
    return complete


if __name__ == '__main__':
    main()
