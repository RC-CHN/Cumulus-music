#!/usr/bin/env python3
"""Read-only verification of GUI stages and durable refresh journals."""
import argparse
import json
from pathlib import Path
import sqlite3

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('root', type=Path)
root = parser.parse_args().root.resolve()
run = root / 'first'
expected = [
    ('renewal_missing', 0, {'missing_credentials'}),
    ('renewal_success', 1, {'refreshed', 'not_due'}),
    ('renewal_daily', 1, {'not_due'}),
    ('renewal_unknown', 2, {'recovery_required', 'reauthentication_required'}),
    ('renewal_relogin', 2, {'reauthentication_required'}),
]
for name, count, states in expected:
    raw = (run / (name + '.json')).read_text()
    data = json.loads(raw)
    assert data['state'] in states and data['nativeRenewal']['state'] in states
    assert data['rotationRequests'] == count and data['realMusicNetworkRequests'] == 0
    assert not data['overflow'] and (run / (name + '.png')).is_file()
    for secret in ['MUSIC_U', 'fixture-refresh', 'fixture-password', '_sign', 'deviceId']:
        assert secret not in raw
capture = json.loads((run / 'capture.json').read_text())
assert capture['ok'] and capture['screenshots'] == 5
complete = json.loads((run / 'complete.json').read_text())
assert complete['ok'] and complete['rotationRequests'] == 2
journals = []
for path in (root / 'state').rglob('core.sqlite3'):
    with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as db:
        for owner, value in db.execute("SELECT owner,value FROM state WHERE kind='login_refresh' AND key='jsk'"):
            assert owner == '2'
            journals.append(json.loads(value))
assert len(journals) == 2
assert sum(j['pending'] for j in journals) == 1
assert sum(not j['pending'] and j['last_success'] is not None for j in journals) == 1
assert all(j['last_success'] is None for j in journals if j['pending'])
result = {'ok': True, 'syntheticOnly': True, 'realMusicRequests': 0,
          'screenshots': 5, 'missingCredentialsSentNothing': True,
          'dailyCheckDidNotRotate': True, 'unknownWriteNotReplayed': True,
          'successfulAndPendingJournalsDurable': True, 'publicStagesExcludeSecrets': True}
(root / 'independent-audit.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result))
