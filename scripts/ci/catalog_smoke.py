"""Actual WebView catalog navigation using synthetic account/API data only."""
import argparse
import json
import os
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    for variable, directory in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                                ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'),
                                ('LOCALAPPDATA', 'local')]:
        path = root / 'os-profile' / directory
        path.mkdir(parents=True)
        env[variable] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([str(args.binary.resolve())], env=env,
                                stdout=log, stderr=log, timeout=240)
    output = root / 'run' / 'first'
    if (output / 'failed.json').exists():
        raise AssertionError((output / 'failed.json').read_text(encoding='utf-8'))
    assert result.returncode == 0, f'catalog probe exited {result.returncode}'
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['stageCount'] == 18 and complete['realMusicRequests'] == 0
    assert complete['syntheticMembershipWrites'] and not complete['playbackVerified']
    stages = [json.loads(p.read_text(encoding='utf-8')) for p in output.glob('ux_*.json')]
    assert len(stages) == 18
    assert {v['theme'] for v in stages} == {'light', 'dark'}
    assert any(v['width'] == 760 and v['height'] == 540 for v in stages)
    assert all(v['rows'] <= 30 and v['cards'] <= 30 for v in stages)
    print(json.dumps({'ok': True, 'actualWebView': True, 'states': 18,
                      'realMusicRequests': 0, 'playbackVerified': False}))


if __name__ == '__main__':
    main()
