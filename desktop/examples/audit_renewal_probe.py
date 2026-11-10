#!/usr/bin/env python3
"""Read-only verification of GUI stages and durable refresh journals."""
import argparse
import json
from pathlib import Path
import sqlite3

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('root', type=Path)
parser.add_argument('--no-screenshots', action='store_true')
parser.add_argument('--foreground', action='store_true')
args = parser.parse_args()
root = args.root.resolve()
run = root / 'first'
identity = '/api/w/nuser/account/get'
forbidden = ('MUSIC_U', 'fixture-refresh', 'fixture-password', '_sign', 'deviceId')


def saved_renewal_journals():
    journals = []
    for path in (root / 'state').rglob('core.sqlite3'):
        with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True) as db:
            for owner, value in db.execute("SELECT owner,value FROM state WHERE kind='login_refresh' AND key='jsk'"):
                assert owner == '2'
                journals.append(json.loads(value))
    return journals


if args.foreground:
    expected = [
        ('fresh', 0, 'not_due', True),
        ('restore_only', 0, 'ready', True),
        ('blurred', 0, 'ready', False),
        ('refreshed', 1, 'refreshed', True),
    ]
    stages = {}
    for suffix, writes, state, focused in expected:
        name = 'renewal_foreground_' + suffix
        raw = (run / (name + '.json')).read_text(encoding='utf-8')
        row = stages[suffix] = json.loads(raw)
        assert row['state'] == row['nativeRenewal']['state'] == state and row['accountStatus'] == 'authenticated'
        assert row['rotationRequests'] == writes and row['realMusicNetworkRequests'] == 0
        assert row['nativeForegroundEligible'] and row['nativeMainForeground'] is focused
        assert row['rendererFocused'] is focused and not row['overflow']
        assert '/api/middle/account/token/refresh' not in row['fixtureRequests']
        assert '/api/login/token/refresh' not in row['fixtureRequests']
        assert not any(secret in raw for secret in forbidden)
        if not args.no_screenshots:
            assert (run / (name + '.png')).is_file()
    fresh, restored, blurred, refreshed = (stages[key] for key in ('fresh', 'restore_only', 'blurred', 'refreshed'))
    assert fresh['nativeFocusEvents'] == restored['nativeFocusEvents'] and restored['nativeFocusEvents'][-1] is True
    assert blurred['nativeFocusEvents'] == restored['nativeFocusEvents'] + [False]
    assert refreshed['nativeFocusEvents'] == blurred['nativeFocusEvents'] + [True]
    assert len({row['nativeRenewal']['epoch'] for row in stages.values()}) == 1
    assert restored['fixtureRequests'][identity] == fresh['fixtureRequests'][identity] + 1
    assert blurred['fixtureRequests'] == restored['fixtureRequests']
    assert refreshed['fixtureRequests'][identity] == blurred['fixtureRequests'][identity] + 1
    assert refreshed['fixtureRequests']['/api/login/token/refresh/v1'] == 1
    complete = json.loads((run / 'complete.json').read_text())
    assert complete['ok'] and complete['rotationRequests'] == 1
    journal, = saved_renewal_journals()
    assert not journal['pending'] and journal['last_success'] is not None
    capture = json.loads((run / 'capture.json').read_text())
    screenshots = 0 if args.no_screenshots else len(expected)
    assert capture['ok'] and capture['screenshots'] == screenshots
    result = {'ok': True, 'syntheticOnly': True, 'realMusicRequests': 0, 'screenshots': screenshots,
              'alreadyFocusedAccountRestoreReadDidNotRotate': True,
              'nativeBlurDidNotRotate': True, 'genuineNativeRefocusRotatedExactlyOnce': True,
              'sameEpochIdentityConfirmed': True}
    (root / 'independent-audit.json').write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result))
    raise SystemExit(0)

expected = [
    ('renewal_missing', 0, {'missing_credentials'}),
    ('renewal_login', 0, {'not_due'}),
    ('renewal_due', 0, {'ready'}),
    ('renewal_identity_pending', 1, {'recovery_required'}),
    ('renewal_success', 1, {'refreshed', 'not_due'}),
    ('renewal_daily', 1, {'not_due'}),
    ('renewal_due_again', 1, {'ready'}),
    ('renewal_second_login', 1, {'not_due'}),
    ('renewal_unconfirmed', 2, {'unconfirmed'}),
    ('renewal_relogin_cancelled', 2, {'unconfirmed'}),
]
stages = {}
for name, count, states in expected:
    raw = (run / (name + '.json')).read_text()
    data = json.loads(raw)
    stages[name] = data
    assert data['state'] in states and data['nativeRenewal']['state'] in states
    assert data['rotationRequests'] == count and data['realMusicNetworkRequests'] == 0
    assert data['accountStatus'] == 'authenticated'
    if name in ['renewal_login', 'renewal_due', 'renewal_second_login']:
        assert data['nativeRenewal']['lastSuccess'] is None
    if name in ['renewal_success', 'renewal_daily', 'renewal_due_again']:
        assert data['nativeRenewal']['lastSuccess'] is not None
    assert not data['overflow']
    if not args.no_screenshots:
        assert (run / (name + '.png')).is_file()
    for secret in forbidden:
        assert secret not in raw
pending = stages['renewal_identity_pending']
success = stages['renewal_success']
assert pending['nativeRenewal']['canReconcile'] and not pending['nativeRenewal']['canCheck']
assert success['fixtureRequests'][identity] == pending['fixtureRequests'][identity] + 1
unknown = stages['renewal_unconfirmed']
cancelled = stages['renewal_relogin_cancelled']
assert not unknown['nativeRenewal']['canCheck'] and not unknown['nativeRenewal']['canReconcile']
assert unknown['nativeRenewal']['epoch'] == cancelled['nativeRenewal']['epoch']
assert unknown['fixtureRequests'][identity] == cancelled['fixtureRequests'][identity]
assert all('/api/middle/account/token/refresh' not in row['fixtureRequests'] for row in stages.values())
capture = json.loads((run / 'capture.json').read_text())
screenshots = 0 if args.no_screenshots else len(expected)
assert capture['ok'] and capture['screenshots'] == screenshots
complete = json.loads((run / 'complete.json').read_text())
assert complete['ok'] and complete['rotationRequests'] == 2
journals = saved_renewal_journals()
assert len(journals) == 2
assert sum(j['pending'] for j in journals) == 1
assert sum(not j['pending'] and j['last_success'] is not None for j in journals) == 1
assert all(j['last_success'] is None for j in journals if j['pending'])
result = {'ok': True, 'syntheticOnly': True, 'realMusicRequests': 0,
          'screenshots': screenshots, 'missingCredentialsSentNothing': True,
          'freshLoginDidNotRotateOrClaimRefresh': True,
          'readinessChangedAtDailyBoundaryWithoutRotation': True,
          'pairedReceiptReconciledWithOneIdentityRead': True,
          'optionalReloginCancellationPreservedActiveSession': True,
          'dailyCheckDidNotRotate': True, 'unknownWriteNotReplayed': True,
          'successfulAndPendingJournalsDurable': True, 'publicStagesExcludeSecrets': True}
(root / 'independent-audit.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result))
