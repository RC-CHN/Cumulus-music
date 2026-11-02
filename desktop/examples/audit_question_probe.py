#!/usr/bin/env python3
"""Read-only SQLite audit of question_login_ui_probe's synthetic accounts."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


def records(path):
    with sqlite3.connect(path.resolve().as_uri()+'?mode=ro',uri=True) as db:
        return {(a,b,c):json.loads(v) for a,b,c,v in db.execute('SELECT owner,kind,key,value FROM state')}


def audit(root,legacy):
    complete=json.loads((root/'first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['realMusicRequests']==0 and complete['stageCount']==25
    states={s['stage'].split('_',2)[2]:s for p in(root/'first').glob('ux_*.json') if(s:=json.loads(p.read_text(encoding='utf-8')))}
    assert len(states)==25
    assert states['verify_unknown_again_small_light']['nativeAccount']['epoch']!=states['reconciled_light']['nativeAccount']['epoch']
    registry=records(root/'state/core.sqlite3')
    selected=registry[('transport','profiles','active')]['session_id']
    saved=[v for(_,kind,_),v in registry.items() if kind=='saved-profiles-v1']
    assert len(saved)==1 and saved[0]['uid']=='4' and saved[0]['session_id']==selected
    rows=[]
    for path in(root/'state/sessions').glob('*/core.sqlite3'):
        record=records(path).get(('transport','credentials','session'))
        if record is not None:rows.append((path.parent.name,record))
    active=next(v for sid,v in rows if sid==selected)
    assert active['refresh_token'] and active['login_rotation']
    assert bool(active.get('last_login_at')) is not legacy
    has_user=lambda v:any(c['raw_cookie'].startswith('MUSIC_U=') for c in v['cookies'])
    assert sum(has_user(v) and bool(v['refresh_token']) for _,v in rows)==1
    partial=[v for _,v in rows if has_user(v) and not v['refresh_token']]
    assert len(partial)==3 and all(not v.get('last_login_at') for v in partial)
    for path in(root/'state').rglob('core.sqlite3'):
        value=json.dumps(list(records(path).values()),ensure_ascii=False)
        assert all(s not in value for s in ['fixture-password',hashlib.md5(b'fixture-password').hexdigest(),'用于检查长文本排布'])
    for path in(root/'first').glob('*.json'):
        raw=path.read_text(encoding='utf-8')
        assert all(s not in raw for s in ['MUSIC_U','fixture-refresh','fixture-password'])
    report=dict(ok=True,syntheticOnly=True,realMusicRequests=0,legacy=legacy,profiles=len(rows),confirmedPairs=1,unconfirmedCookiesWithoutRefresh=3,savedAccountDeduplicated=True,selectedCredentialsDurable=True,answerNotPersisted=True,newEpochAfterReconciliation=True)
    (root/'independent-audit.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    return report


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',type=Path);p.add_argument('--legacy',action='store_true');args=p.parse_args()
    print(json.dumps(audit(args.root.resolve(),args.legacy)))
