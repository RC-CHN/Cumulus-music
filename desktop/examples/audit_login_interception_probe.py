#!/usr/bin/env python3
"""Read-only audit of production UI events and synthetic credential invalidation."""
import argparse
import json
from pathlib import Path
import sqlite3

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('root', type=Path)
root = parser.parse_args().root.resolve()
run = root / 'first'
legacy = json.loads((run / 'complete.json').read_text()).get('legacyPolicy', False)
for name, checks, status in [
    ('restored', 0, 'authenticated'), ('retained', 1, 'authenticated'),
    ('expired', 2, 'expired'), ('login', 2, 'expired'),
]:
    path = run / ('interception_' + name + '.json')
    raw = path.read_text()
    row = json.loads(raw)
    assert row['nativeAccount']['status'] == status
    assert row['validationRequests'] == checks and row['searchRequests'] == checks
    assert row['realMusicNetworkRequests'] == 0 and path.with_suffix('.png').is_file()
    assert row['fixtureRequests'].get('/api/login/token/refresh/v1', 0) == 0
    if legacy:
        assert row['queueCounts'] == [0 if status == 'expired' else 1, 1, 1]
    for secret in ['MUSIC_U', 'fixture-refresh', 'fixture-password', '_sign', 'deviceId']:
        assert secret not in raw
    if status == 'expired':
        notice = row['nativeAccount']['loginNotice']
        assert notice == {'kind': 'reauthenticate', 'title': '请重新确认登录', 'message': '登录状态已失效，请重新登录'}
with sqlite3.connect((root / 'profile/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
    record = json.loads(db.execute("SELECT value FROM state WHERE owner='transport' AND kind='credentials' AND key='session'").fetchone()[0])
    cached = db.execute("SELECT value FROM state WHERE owner='2' AND kind='login-policy-v1' AND key='pc'").fetchone()
    assert cached is not None and json.loads(cached[0]) == {'jsk': not legacy, 'benefit': not legacy}
assert record['cookies'] == [] and record['refresh_token'] is None and record['login_rotation'] is None
assert record['login_notice']['kind'] == 'reauthenticate'
if legacy:
    assert record.get('queue_reset') is None
    with sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        receipts = db.execute("SELECT value FROM state WHERE owner='2' AND kind='login-queue-reset-v1'").fetchall()
        assert receipts == [('"cleared"',)]
assert json.loads((run / 'complete.json').read_text())['ok']
assert json.loads((run / 'capture.json').read_text())['ok']
result = {'ok': True, 'syntheticOnly': True, 'realMusicRequests': 0,
          'screenshots': 4, 'failedSearchesNotReplayed': True,
          'validTokenRetainedAccount': True, 'explicitInvalidationClearedCredentials': True,
          'nativeEventReachedUi': True, 'reloginEntryOpened': True,
          'experimentAssignmentPersisted': True, 'legacyPolicy': legacy,
          'serverQueueClearVerified': legacy}
(root / 'independent-audit.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result))
