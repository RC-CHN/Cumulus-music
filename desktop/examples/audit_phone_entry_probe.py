"""Read-only SQLite audit of synthetic ordinary signup and unknown-write recovery."""
import argparse
import json
import sqlite3
from pathlib import Path


def records(path):
    with sqlite3.connect(path.resolve().as_uri()+'?mode=ro', uri=True) as db:
        return {(o, k, key): json.loads(v) for o, k, key, v in db.execute('SELECT owner,kind,key,value FROM state')}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('root', type=Path)
    p.add_argument('--legacy', action='store_true')
    args = p.parse_args()
    root = args.root.resolve()
    done = json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert done['ok'] and done['phoneEntry']
    registry = records(root/'state/core.sqlite3')
    assert not any(kind == 'phone-registration-v1' for _, kind, _ in registry)
    saved = {v['uid']: v for (_, kind, _), v in registry.items() if kind == 'saved-profiles-v1'}
    assert set(saved) == {'2', '5'}
    selection = registry[('transport', 'profiles', 'active')]
    assert selection['session_id'] == saved['5']['session_id'] and not selection['auto_login']
    assert saved['5']['auto_login'] is False
    active = records(root/'state/sessions'/selection['session_id']/'core.sqlite3')[('transport', 'credentials', 'session')]
    assert active['refresh_token'] == 'fixture-refresh'
    assert any(c['raw_cookie'].startswith('MUSIC_U=fixture5;') for c in active['cookies'])
    assert bool(active.get('last_login_at')) != args.legacy
    checked = 0
    for path in (root/'state').rglob('core.sqlite3'):
        values = json.dumps(list(records(path).values()), ensure_ascii=False)
        for text in ['123456', '13800138000', 'fixture-password', '重复昵称', '关闭清空昵称', '明确新昵称',
                     '查重失败昵称', '推荐失败保留昵称', '挂起昵称']:
            assert json.dumps(text, ensure_ascii=False) not in values, 'credential input persisted'
        checked += 1
    for path in (root/'first').glob('*.json'):
        value = path.read_text(encoding='utf-8')
        assert all(secret not in value for secret in ['MUSIC_U=', 'fixture-refresh', 'fixture-partial-must-not-install', 'fixture-password'])
    report = dict(ok=True, syntheticOnly=True, databasesChecked=checked, unconfirmedRegistrationSettledByFreshLogin=True,
                  explicitDefaultAndNamedRegistration=True, autoLoginPreserved=True, inputNotPersisted=True,
                  realTokenIssuanceProven=False, realMusicRequests=0, realAccountWrites=0)
    (root/'independent-audit.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
