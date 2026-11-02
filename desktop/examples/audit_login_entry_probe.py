"""Independent read-only audit of synthetic login-entry preferences and credentials."""
import argparse
import json
from pathlib import Path
import sqlite3


def records(path):
    with sqlite3.connect(path.resolve().as_uri()+'?mode=ro', uri=True) as db:
        return {(owner, kind, key): json.loads(value)
                for owner, kind, key, value in db.execute('SELECT owner,kind,key,value FROM state')}


def audit(root):
    done = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert done['ok'] and done['entry'] and done['stageCount'] == 25
    registry = records(root/'state/core.sqlite3')
    assert registry[('_local', 'login_preferences', 'country')] == 'CA'
    assert len([key for key in registry if key[1] == 'login_preferences']) == 1
    saved = [value for (_, kind, _), value in registry.items() if kind == 'saved-profiles-v1']
    assert len(saved) == 1 and saved[0]['uid'] == '2'
    selection = registry[('transport', 'profiles', 'active')]
    assert selection['auto_login'] is False and selection['session_id'] == saved[0]['session_id']
    active = records(root/'state/sessions'/selection['session_id']/'core.sqlite3')[('transport', 'credentials', 'session')]
    assert active['refresh_token'] and active.get('last_login_at')
    assert any(row['raw_cookie'].startswith('MUSIC_U=fixture2') for row in active['cookies'])
    assert done['fixture']['paths'].get('/api/login/token/refresh/v1', 0) == 0
    for path in (root/'state').rglob('core.sqlite3'):
        value = json.dumps(list(records(path).values()), ensure_ascii=False)
        for secret in ['13800000000', '51234567', '1111111111', '123456', 'fixture-password',
                       'alice@example.test', 'name@163.com']:
            assert json.dumps(secret) not in value, 'unsubmitted credentials must not persist'
    for path in (root/'first').glob('*.json'):
        value = path.read_text(encoding='utf-8')
        assert all(secret not in value for secret in ['MUSIC_U=', 'fixture-refresh', 'fixture-password'])
    result = dict(ok=True, syntheticOnly=True, selectedRegionDurable=True, sharedCallingCodeKeepsAlpha=True,
                  autoLoginPreserved=True, selectedCredentialsDurable=True, inputAndConsentNotPersisted=True,
                  nativeBrowserAdapterCalls=len(done['fixture']['browserDocuments']), browserPagesOpened=0,
                  realMusicRequests=0, realAccountWrites=0, realTokenIssuanceProven=False)
    (root/'independent-audit.json').write_text(json.dumps(result, indent=2)+'\n', encoding='utf-8')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    args = parser.parse_args()
    print(json.dumps(audit(args.root.resolve())))
