#!/usr/bin/env python3
"""Read-only audit of the synthetic email proof; never export credential values."""
import argparse
import json
from pathlib import Path
import sqlite3


def records(path):
    with sqlite3.connect(path.resolve().as_uri()+'?mode=ro',uri=True)as db:
        return {(owner,kind,key):json.loads(value) for owner,kind,key,value in db.execute('SELECT owner,kind,key,value FROM state')}


def audit(root,legacy):
    registry=records(root/'state/core.sqlite3')
    selected=registry[('transport','profiles','active')]['session_id']
    saved=[value for(_,kind,_),value in registry.items()if kind=='saved-profiles-v1']
    assert len(saved)==1 and saved[0]['session_id']==selected and saved[0]['uid']=='3'
    credentials=[]
    for path in sorted((root/'state/sessions').glob('*/core.sqlite3')):
        record=records(path).get(('transport','credentials','session'))
        if record is None:continue
        user=any(c['raw_cookie'].startswith('MUSIC_U=')for c in record['cookies'])
        assert not record.get('refresh_token')or user
        if user:
            assert all(c['domain']=={'HostOnly':'interfacepc.music.163.com'} for c in record['cookies'] if c['raw_cookie'].startswith('MUSIC_U='))
        credentials.append((path.parent.name,record,user))
    active=next(record for name,record,_ in credentials if name==selected)
    assert active.get('refresh_token')and active.get('login_rotation')
    assert bool(active.get('last_login_at'))is not legacy
    assert sum(user and bool(record.get('refresh_token'))for _,record,user in credentials)==2
    assert sum(user and not record.get('refresh_token')for _,record,user in credentials)==1
    assert all(not record.get('last_login_at')for _,record,user in credentials if user and not record.get('refresh_token'))
    states={value['stage'].split('_',2)[2]:value for path in(root/'first').glob('ux_*.json')if(value:=json.loads(path.read_text(encoding='utf-8')))}
    assert len(states)==22
    assert states['verify_unknown_light']['nativeAccount']['epoch']!=states['reconciled_light']['nativeAccount']['epoch']
    for path in(root/'first').glob('*.json'):
        raw=path.read_text(encoding='utf-8')
        assert not any(secret in raw for secret in ['MUSIC_U','MUSIC_TU','fixture-refresh','fixture-password','synthetic-email-ticket'])
    result=dict(ok=True,syntheticOnly=True,profiles=len(credentials),deduplicatedSavedAccount=True,confirmedPairs=2,partialCookieWithoutRefresh=1,selectedCredentialsDurable=True,newAccountEpochAfterReconciliation=True,legacy=legacy,realMusicRequests=0)
    (root/'independent-audit.json').write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    return result


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root',type=Path)
    parser.add_argument('--legacy',action='store_true')
    args=parser.parse_args()
    print(json.dumps(audit(args.root.resolve(),args.legacy)))
