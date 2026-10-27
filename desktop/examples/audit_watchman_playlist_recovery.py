#!/usr/bin/env python3
"""Offline audit: reopen a completed playlist workflow without mutation replay."""
import hashlib
import json
from pathlib import Path
import sqlite3
import sys

original, recovered = map(lambda s: Path(s).resolve(), sys.argv[1:])
inputs = []
def read(root, name):
    data = (root / name).read_bytes()
    inputs.append(dict(root='original' if root == original else 'recovered', path=name,
                       sha256=hashlib.sha256(data).hexdigest()))
    return json.loads(data)

plan = read(original, 'playlist-plan.json')
result = read(original, 'playlist-result.json')
complete = read(recovered, 'complete.json')
assert result['ok'] and result['workflowPassed'] and result['businessWrites'] == 9
assert complete['ok'] and complete['businessWrites'] == 0
assert complete['playlistAudit']['recoveredOriginalJournal'] and complete['playlistAudit']['baselineRestored']
files = sorted(original.glob('playlist-request-*.json'), key=lambda p: int(p.stem.rsplit('-', 1)[1]))
assert len(files) == 48
intents = []
for i, file in enumerate(files):
    q = read(original, file.name)
    intents.append(q['intent'])
    if i < 45:
        continue
    r = read(original, file.name.replace('request', 'response'))
    assert q['intent'] == 'read' and q['route'] == 'eapi'
    assert r['status'] == 200 and r['body']['code'] == 200
    if i == 45:
        assert q['path'] == '/api/w/nuser/account/get'
        assert str(r['body']['profile']['userId']) == plan['owner']
    else:
        assert q['path'] == '/api/user/playlist' and q['payload']['uid'] == plan['owner']
        assert not r['body']['more']
        assert [str(p['id']) for p in r['body']['playlist']] == [p['id'] for p in plan['baseline']['playlists']]
assert len([i for i in intents if i != 'read']) == 9 and 'cleanup' not in intents
assert read(original, 'playlist-recovery-0/result.json') == complete['playlistAudit']
assert read(original, 'playlist-recovery-0/list-0.json')['playlists'] == plan['baseline']['playlists']
database = original / 'playlist-state/core.sqlite3'
with sqlite3.connect(database.as_uri()+'?mode=ro', uri=True) as db:
    rows = db.execute("SELECT owner,value FROM state WHERE kind='playlist-mutations'").fetchall()
assert len(rows) == 11 and all(owner == plan['owner'] for owner, _ in rows)
records = [json.loads(v) for _, v in rows]
assert len([r for r in records if r['phase'] == 'confirmed']) == 9
assert len([r for r in records if r['phase'] == 'unchanged']) == 2
assert {r['operation_id'] for r in records} == {
    read(original, 'stage-'+stage+'.json')['operation_id'] for stage in result['stages']}
inputs.append(dict(root='original',path='playlist-state/core.sqlite3',sha256=hashlib.sha256(database.read_bytes()).hexdigest()))
summary = dict(ok=True,originalOperations=11,recoveryReads=3,newWriteRequests=0,
               newBusinessOperations=0,baselineRestored=True,networkRequestsByAudit=0)
with (recovered / 'recovery-audit.json').open('x') as f:
    json.dump(dict(**summary,inputs=inputs),f,indent=2)
    f.write('\n')
print(json.dumps(summary))
