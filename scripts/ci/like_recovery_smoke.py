"""Main-window likes: real UI/IPC, synthetic remote failures, process restart."""
import argparse
import json
import os
from pathlib import Path
import sqlite3

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
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_LIKE_RECOVERY_PROOF='1')
    for key, name in [
        ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
        ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
        ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'),
    ]:
        folder = root / 'os-profile' / name
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    for phase, count, writes in [('first', 11, 4), ('reopen', 3, 0)]:
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            status = launch(args.binary.resolve(), env, log, output, args.screenshots)
        if (output / 'failed.json').exists():
            raise AssertionError((output / 'failed.json').read_text(encoding='utf-8'))
        assert status == 0, status
        result = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert result['ok'] and result['stageCount'] == count and result['realMusicRequests'] == 0
        assert result['syntheticLikeWrites'] == writes, result
        assert len(list(output.glob('ux_*.json'))) == count
        if args.screenshots:
            assert len(list(output.glob('ux_*.png'))) == count
    with sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        records = {
            key: json.loads(value) for key, value in db.execute(
                "SELECT key,value FROM state WHERE owner='2' AND kind='resource-mutations'"
            )
        }
        assert all(records[f'liked:{id}']['phase'] == 'confirmed' for id in [2, 4, 5])
        history = [json.loads(value) for value, in db.execute(
            "SELECT value FROM state WHERE owner='2' AND kind='resource-recovery-history-v1'"
        )]
        assert len(history) == 1 and history[0]['resolution']['phase'] == 'accepted_current_state'
        assert history[0]['original']['desired'] is True
        assert history[0]['resolution']['observed'] is False
    report = {
        'ok': True, 'actualWebView': True, 'states': 14,
        'screenshots': 14 if args.screenshots else 0, 'realMusicRequests': 0,
        'syntheticLikeWrites': 4, 'lostAckOnlyInspected': True,
        'pendingRestoredAfterRestart': True, 'acceptedHistoryPreserved': True,
    }
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
