"""Actual WebView + production UI/IPC, synthetic login lifecycle, no account/network."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--legacy', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    assert not root.exists(), 'proof directory must be new'
    root.parent.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_PROOF_SMALL='1')
    if args.legacy:
        env['CUMULUS_PROOF_LEGACY'] = '1'
    with root.with_suffix('.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([str(args.binary.resolve())], env=env, stdout=log, stderr=log, timeout=180)
    assert result.returncode == 0, f'UI probe exited {result.returncode}; inspect {root.with_suffix(".log")}'
    run = root / 'first'
    complete = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] is True and complete['legacyPolicy'] == args.legacy
    for stage, count, status in [('restored', 0, 'authenticated'), ('retained', 1, 'authenticated'), ('expired', 2, 'expired'), ('login', 2, 'expired')]:
        raw = (run / f'interception_{stage}.json').read_text(encoding='utf-8')
        row = json.loads(raw)
        assert row['nativeAccount']['status'] == status
        assert row['searchRequests'] == row['validationRequests'] == count
        assert row['realMusicNetworkRequests'] == 0
        assert row['fixtureRequests'].get('/api/login/token/refresh/v1', 0) == 0
        assert all(secret not in raw for secret in ['MUSIC_U', 'fixture-refresh', 'fixture-password', '_sign', 'deviceId'])
        if status == 'expired':
            assert row['nativeAccount']['loginNotice'] == {'kind': 'reauthenticate', 'title': '请重新确认登录', 'message': '登录状态已失效，请重新登录'}
        if args.legacy:
            assert row['queueCounts'] == [0 if status == 'expired' else 1, 1, 1]
    with sqlite3.connect((root / 'profile/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        credentials = json.loads(db.execute("SELECT value FROM state WHERE owner='transport' AND kind='credentials' AND key='session'").fetchone()[0])
    assert credentials['cookies'] == [] and credentials['refresh_token'] is None and credentials['login_rotation'] is None
    assert credentials['login_notice']['kind'] == 'reauthenticate'
    if args.legacy:
        assert credentials.get('queue_reset') is None
        with sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
            assert db.execute("SELECT value FROM state WHERE owner='2' AND kind='login-queue-reset-v1'").fetchall() == [('"cleared"',)]
    proof = {'ok': True, 'platform': os.name, 'legacy': args.legacy, 'domStages': 4,
             'actualWebView': True, 'actualProductionUiAndIpc': True, 'syntheticHttpOnly': True,
             'realMusicRequests': 0, 'screenshotsCaptured': False, 'credentialsInvalidated': True}
    (root / 'ci-ui-audit.json').write_text(json.dumps(proof, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(proof))


if __name__ == '__main__':
    main()
