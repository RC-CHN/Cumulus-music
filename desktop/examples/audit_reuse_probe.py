#!/usr/bin/env python3
"""Independent read-only persistence audit; only synthetic phone-reuse profiles."""
import argparse
import json
from pathlib import Path
import sqlite3


def records(path):
    with sqlite3.connect(path.resolve().as_uri()+'?mode=ro',uri=True) as db:
        return {(a,b,c):json.loads(v) for a,b,c,v in db.execute('SELECT owner,kind,key,value FROM state')}


def audit(root):
    complete=json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['stageCount']==34 and complete['realMusicRequests']==0
    states={s['stage'].split('_',2)[2]:s for p in(root/'first').glob('ux_*.json') if(s:=json.loads(p.read_text(encoding='utf-8')))}
    registry=records(root/'state/core.sqlite3')
    assert not any(kind=='phone-registration-v1' for _,kind,_ in registry)
    assert len([v for (_,kind,_),v in registry.items() if kind=='phone-registration-seed-v1'])==1
    selection=registry[('transport','profiles','active')]
    assert selection['auto_login'] is False
    saved=[v for (_,kind,_),v in registry.items() if kind=='saved-profiles-v1']
    assert {v['uid'] for v in saved}=={'2','3'}
    assert next(v for v in saved if v['uid']=='3')['session_id']==selection['session_id']
    active=records(root/'state/sessions'/selection['session_id']/'core.sqlite3')[('transport','credentials','session')]
    assert active['refresh_token'] and active['login_rotation'] and not active.get('last_login_at')
    assert any(c['raw_cookie'].startswith('MUSIC_U=fixture3') for c in active['cookies'])
    unknown=states['registration_unknown_small_light']['nativeAccount']['epoch']
    reconciled=states['registration_reconciled_light']['nativeAccount']['epoch']
    assert unknown!=reconciled
    assert states['register_inflight_dark']['nativeAccount']['epoch']==states['retry_register_blocked_dark']['nativeAccount']['epoch']
    assert states['fresh_sms_form_light']['nativeAccount']['epoch']!=states['fresh_sms_resolved_light']['nativeAccount']['epoch']
    for path in(root/'state').rglob('core.sqlite3'):
        value=json.dumps(list(records(path).values()),ensure_ascii=False)
        assert all(json.dumps(secret,ensure_ascii=False) not in value for secret in ['13800138000','123456','最终合成昵称','关闭清空昵称','重复昵称'])
    for path in(root/'first').glob('*.json'):
        value=path.read_text(encoding='utf-8')
        assert all(secret not in value for secret in ['MUSIC_U=','fixture-refresh','123456','DO_NOT_EXPORT'])
    result=dict(ok=True,syntheticOnly=True,realMusicRequests=0,selectedCredentialsDurable=True,autoLoginPreserved=True,savedAccounts=2,registrationIntentResolved=True,phoneOtpAndNicknameDraftNotPersisted=True,registrationDidNotFabricateJskLoginTime=True)
    (root/'independent-audit.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',type=Path);args=p.parse_args();print(json.dumps(audit(args.root.resolve())))
