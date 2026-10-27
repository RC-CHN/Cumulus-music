#!/usr/bin/env python3
"""Independently audit Rust device files via OpenSSL and private request contexts.

No network, no Cookie extraction, no hardware values printed in the report.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3
import struct
import subprocess


def read_state(root, owner, kind, key):
    path = (root/'core.sqlite3').resolve().as_uri() + '?mode=ro'
    with sqlite3.connect(path, uri=True) as db:
        row = db.execute('SELECT value FROM state WHERE owner=? AND kind=? AND key=?', (owner,kind,key)).fetchone()
        assert row
        return json.loads(row[0])


def decode_record(data):
    encrypted = data[16:]
    count = len(encrypted)//16*16
    plain = subprocess.check_output(['openssl','enc','-aes-128-cbc','-d','-nopad',
        '-K','456789f1124577c2b1459012429035e2','-iv','00'*16],input=encrypted[:count])
    tail=encrypted[count:]
    if tail:
        assert len(tail)>1
        rest=bytearray(len(tail));rest[-1]=tail[-1]^tail[0]
        for i in range(len(tail)-2,-1,-1):rest[i]=tail[i]^rest[i+1]
        plain+=rest
    assert hashlib.md5(plain).digest()==data[:16]
    return plain


def audit(offline, live, profile):
    data=(offline/'isc.dat').read_bytes()
    assert data==(offline/'reexport.dat').read_bytes()
    assert data[:8]==bytes.fromhex('5046445700003c00')
    assert hashlib.md5(data[24:]).digest()==data[8:24]
    count=struct.unpack_from('<I',data,24)[0];assert count==2
    records={}
    end=28+12*count
    for i in range(count):
        key,offset,size=struct.unpack_from('<III',data,28+12*i)
        assert offset==end and offset+size<=len(data)
        records[key]=decode_record(data[offset:offset+size]);end=offset+size
    assert end==len(data) and set(records)=={0x90001,0x1001}
    identity=read_state(offline,'device','identity','client_sign')['persistent_id']
    assert records[0x90001]==identity.encode('ascii') and len(identity)==64
    assert hashlib.md5(identity[:32].encode()).hexdigest()==identity[32:]
    assert records[0x1001]==b'b6dec0e1-fa12-4698-a3e0-321498e274284b5b1d41b26801663ffe0306cfbef394'
    assert read_state(offline/'imported','device','identity','client_sign')['persistent_id']==identity
    live_id=read_state(live/'identity','device','identity','client_sign')['persistent_id']
    parts=read_state(live/'identity','device','identity','last_observed')['parts']
    assert len(parts)==4 and parts[-1]==live_id
    sign=''
    for part in parts:
        if sign:sign+='@'
        sign+=part
    requests=[json.loads(p.read_text()) for p in sorted(live.glob('request-*.json'))]
    responses=[json.loads(p.read_text()) for p in sorted(live.glob('response-*.json'))]
    assert len(requests)==len(responses)==2
    assert all(v=={'status':200,'code':200} for v in responses)
    assert requests[0]==requests[1]
    live_report=json.loads((live/'report.json').read_text())
    windows=live_report.get('declaredWindowsCompatibility',False)
    if windows:
        fixture=json.loads(Path('core/tests/fixtures/windows-compatibility-native.json').read_text())
        expected_osver=fixture['descriptionCases'][0]['output']
        assert all(v['context']['userAgent']==fixture['officialUserAgent'] for v in requests)
    else:
        expected_osver='Linux'
    assert all(v['path']=='/api/w/nuser/account/get' and v['context']['clientSign']==sign and v['context']['osver']==expected_osver for v in requests)
    # Fetch only the original credential device identifier, never Cookie data.
    with sqlite3.connect((profile/'core.sqlite3').resolve().as_uri()+'?mode=ro',uri=True) as db:
        device=db.execute("SELECT json_extract(value,'$.device_id') FROM state WHERE owner='transport' AND kind='credentials' AND key='session'").fetchone()[0]
    assert requests[0]['context']['deviceId']==device and device!=live_id and device!=sign
    report=dict(ok=True,containerRecords=2,independentOpenSsl=True,recordDigests=True,
                exactImportExport=True,realAccountReads=2,mutations=0,
                persistedContextMatchesWire=True,credentialDeviceIdDistinct=True,
                contextStableAcrossReopen=True,officialWindowsGeneratedFileTested=False,
                declaredWindowsCompatibility=windows,realWindowsEnvironment=False)
    with (live/'independent-audit.json').open('x') as stream:stream.write(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('offline',type=Path);p.add_argument('live',type=Path);p.add_argument('profile',type=Path)
    args=p.parse_args();audit(args.offline,args.live,args.profile)
