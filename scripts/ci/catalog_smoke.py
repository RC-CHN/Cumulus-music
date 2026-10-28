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
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--collections', action='store_true')
    mode.add_argument('--history', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    if args.collections:
        env['CUMULUS_COLLECTION_PROOF'] = '1'
    if args.history:
        env['CUMULUS_HISTORY_PROOF'] = '1'
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
    count = 24 if args.history else 18
    assert complete['ok'] and complete['stageCount'] == count and complete['realMusicRequests'] == 0
    assert not complete['playbackVerified']
    if args.history:
        assert complete['syntheticHistoryWrites'] == 2
    else:
        assert complete['collectionReadOnly'] if args.collections else complete['syntheticMembershipWrites']
    stages = [json.loads(p.read_text(encoding='utf-8')) for p in output.glob('ux_*.json')]
    assert len(stages) == count
    assert {v['theme'] for v in stages} == {'light', 'dark'}
    assert any(v['width'] == 760 and v['height'] == 540 for v in stages)
    if args.history:
        assert all(v['rows'] <= 35 and v['footerBottom'] <= v['mainBottom'] + 1 for v in stages)
        assert {v['scope'] for v in stages} == {'歌曲', '播客', '有声书', '声音', '歌单', '专辑'}
    else:
        assert all(v['cards'] <= 30 for v in stages)
    if not args.collections and not args.history:
        assert all(v['rows'] <= 30 for v in stages)
    print(json.dumps({'ok': True, 'actualWebView': True, 'states': count,
                      'realMusicRequests': 0, 'playbackVerified': False,
                      'collections': args.collections, 'history': args.history}))


if __name__ == '__main__':
    main()
