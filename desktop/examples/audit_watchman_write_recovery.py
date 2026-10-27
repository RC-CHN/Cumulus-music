#!/usr/bin/env python3
"""Offline proof of a completed write journal reopened without another write."""
import hashlib
import json
from pathlib import Path
import sqlite3
import sys

original, recovered = map(lambda p: Path(p).resolve(), sys.argv[1:])
inputs = []
def read(root, name):
    data = (root / name).read_bytes()
    inputs.append(dict(root='original' if root == original else 'recovered',
                       path=name, sha256=hashlib.sha256(data).hexdigest()))
    return json.loads(data)

plan = read(original, 'write-plan.json')
initial = read(original, 'write-result.json')
complete = read(recovered, 'complete.json')
assert initial['officialCreateAccepted'] and initial['baselineRestored']
assert complete['ok'] and complete['businessWrites'] == 0
assert complete['writeAudit']['totalWriteIntents'] == 2
assert complete['writeAudit']['baselineRestored'] and not complete['writeAudit']['cleanupSent']
requests = sorted(original.glob('write-request-*.json'), key=lambda p: int(p.stem.rsplit('-', 1)[1]))
# This probe has exactly 9 original requests. Restart adds account + two reads.
assert len(requests) == 12
intents = []
for i, file in enumerate(requests):
    request = read(original, file.name)
    reply = read(original, file.name.replace('request', 'response'))
    intents.append(request['intent'])
    if i < 9:
        continue
    assert request['intent'] == 'read' and request['route'] == 'eapi'
    assert reply['status'] == 200 and reply['body']['code'] == 200
    if i == 9:
        assert request['path'] == '/api/w/nuser/account/get'
        assert str(reply['body']['profile']['userId']) == plan['owner']
    else:
        assert request['path'] == '/api/user/playlist'
        assert str(request['payload']['uid']) == plan['owner'] and not reply['body']['more']
        assert [str(p['id']) for p in reply['body']['playlist']] == [p['id'] for p in plan['baseline']['playlists']]
assert intents.count('isolated_private_create') == intents.count('preselected_cleanup_delete') == 1
recovery = read(original, 'write-recovery-0/write-result.json')
assert recovery == complete['writeAudit']
assert read(original, 'write-recovery-0/write-readback-0.json')['playlists'] == plan['baseline']['playlists']
database = original / 'write-state/core.sqlite3'
with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
    rows = db.execute("SELECT owner,value FROM state WHERE kind='playlist-mutations'").fetchall()
assert len(rows) == 1 and rows[0][0] == plan['owner']
record = json.loads(rows[0][1])
assert record['phase'] == 'confirmed' and record['command'] == dict(kind='create', name=plan['name'], private=True)
inputs.append(dict(root='original', path='write-state/core.sqlite3', sha256=hashlib.sha256(database.read_bytes()).hexdigest()))
summary = dict(ok=True, reopenedOriginalJournal=True, newMusicWriteRequests=0,
               recoveryReads=3, baselineRestored=True, totalCreateIntents=1,
               totalDeleteIntents=1, networkRequestsByAudit=0)
with (recovered / 'recovery-audit.json').open('x') as f:
    json.dump(dict(**summary, inputs=inputs), f, indent=2)
    f.write('\n')
print(json.dumps(summary))
