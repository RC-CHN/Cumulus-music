#!/usr/bin/env python3
"""Read-only audit of production UI events and synthetic credential invalidation."""
import argparse
import json
from pathlib import Path
import sqlite3

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('root', type=Path)
parser.add_argument('--without-screenshots', action='store_true',
                    help='audit the same native and DOM receipts from the Windows runner')
args = parser.parse_args()
root = args.root.resolve()
run = root / 'first'
complete = json.loads((run / 'complete.json').read_text(encoding='utf-8'))
legacy = complete.get('legacyPolicy', False)
ordinary = complete.get('ordinaryJskExpiry', False)
assert not (ordinary and legacy)
states = [('restored', 0, 'authenticated'), ('retained', 1, 'authenticated')]
if ordinary:
    states += [('ordinary_notice', 2, 'expired'), ('ordinary_welcome', 2, 'expired'),
               ('ordinary_retired', 2, 'expired')]
    assert 6500 <= complete['duration'] <= 10000
    assert not complete['classicQrTicketDelayMs'] and complete['classicQrPolls'] == 0
else:
    states += [('expired', 2, 'expired'), ('login', 2, 'expired')]
if legacy:
    states += [('guide_retry', 2, 'expired'), ('guide_closed', 2, 'expired')]
    assert len(complete['classicQrTicketDelayMs']) == 2 and complete['classicQrTicketDelayMs'][0] >= 1000
    assert complete['classicQrPolls'] == complete['pollsBeforeClose'] >= 1
for name, checks, status in states:
    path = run / ('interception_' + name + '.json')
    raw = path.read_text(encoding='utf-8')
    row = json.loads(raw)
    assert row['nativeAccount']['status'] == status
    assert row['validationRequests'] == checks and row['searchRequests'] == checks
    assert row['realMusicNetworkRequests'] == 0
    if not args.without_screenshots:
        assert path.with_suffix('.png').is_file()
    assert row['fixtureRequests'].get('/api/login/token/refresh/v1', 0) == 0
    if legacy:
        assert row['queueCounts'] == [0 if status == 'expired' else 1, 1, 1]
        assert row['guide'] == (name in ('login', 'guide_retry')), name
        assert ('classicRemoveGuide' in row['nativeAccount']) == (name == 'expired'), name
        assert len(row['classicQrTicketDelayMs']) == {'login': 1, 'guide_retry': 2, 'guide_closed': 2}.get(name, 0), name
    else:
        assert not row['guide']
    for secret in ['MUSIC_U', 'fixture-refresh', 'fixture-password', '_sign', 'deviceId']:
        assert secret not in raw
    if status == 'expired':
        notice = row['nativeAccount']['loginNotice']
        if ordinary:
            message = '合成会话已经失效 <img>'
            assert notice == {'kind': 'expired', 'title': '合成失效标题', 'message': message}
            assert row['nativeAccount']['liveJskExpiry'] is True
            if name == 'ordinary_welcome':
                assert not row['localPage'] and not row['localFocus']
                assert row['fallbackTitle'] == '登录已失效' and row['fallbackLogin']
                assert '合成会话已经失效' not in row['text'] and '合成失效标题' not in row['text']
            else:
                assert row['localPage'] and row['localFocus']
            assert row['expiryNotice'] == (None if name == 'ordinary_retired' else message)
        else:
            assert notice == {'kind': 'reauthenticate', 'title': '请重新确认登录', 'message': '登录状态已失效，请重新登录'}
    if not (ordinary and status == 'expired'):
        assert 'liveJskExpiry' not in row['nativeAccount']
        assert row.get('expiryNotice') is None
    if ordinary:
        assert row['fixtureRequests'].get('/api/login/qrcode/unikey', 0) == 0
        assert row['fixtureRequests'].get('/api/login/qrcode/client/login', 0) == 0
with sqlite3.connect((root / 'profile/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
    record = json.loads(db.execute("SELECT value FROM state WHERE owner='transport' AND kind='credentials' AND key='session'").fetchone()[0])
    cached = db.execute("SELECT value FROM state WHERE owner='2' AND kind='login-policy-v1' AND key='pc'").fetchone()
    assert cached is not None and json.loads(cached[0]) == {'jsk': not legacy, 'benefit': not legacy}
assert record['cookies'] == [] and record['refresh_token'] is None and record['login_rotation'] is None
assert record['login_notice']['kind'] == ('expired' if ordinary else 'reauthenticate')
assert 'liveJskExpiry' not in json.dumps(record) and 'live_jsk_expiry' not in json.dumps(record)
if legacy:
    assert record.get('queue_reset') is None
    with sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        receipts = db.execute("SELECT value FROM state WHERE owner='2' AND kind='login-queue-reset-v1'").fetchall()
        assert receipts == [('"cleared"',)]
assert complete['ok']
if not args.without_screenshots:
    assert json.loads((run / 'capture.json').read_text())['ok']
result = {'ok': True, 'syntheticOnly': True, 'realMusicRequests': 0,
          'screenshots': 0 if args.without_screenshots else len(states), 'failedSearchesNotReplayed': True,
          'validTokenRetainedAccount': True, 'explicitInvalidationClearedCredentials': True,
          'nativeEventReachedUi': True, 'reloginEntryOpened': not ordinary,
          'ordinaryJskExpiryNotice': ordinary, 'localRouteAndFocusRetained': ordinary,
          'singleReasonAndManualFallback': ordinary,
          'experimentAssignmentPersisted': True, 'legacyPolicy': legacy,
          'serverQueueClearVerified': legacy}
(root / 'independent-audit.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps(result))
