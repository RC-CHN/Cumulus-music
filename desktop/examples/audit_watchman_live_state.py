#!/usr/bin/env python3
"""Read-only SQLite/reply comparison; emit no device IDs, tokens or account data."""
import hashlib
import json
from pathlib import Path
import sqlite3
import sys

root = Path(sys.argv[1]).resolve()
inputs = []
def read(name):
    data = (root / name).read_bytes()
    inputs.append(dict(path=name, sha256=hashlib.sha256(data).hexdigest()))
    return json.loads(data)

pc = read('pc-config.json')
device = behavior = None
reports = 0
for p in sorted(root.glob('watchman-request-*.json'),key=lambda p:int(p.stem.rsplit('-',1)[1])):
    request = read(p.name)
    if request['path'] in ('/v3/d', '/v3/b'):
        reply = read(p.name.replace('request', 'response'))
        assert reply[0] == 200
        reports += 1
        if request['path'] == '/v3/d':
            device = reply
        else:
            behavior = reply
assert device and behavior
database = root / 'state/core.sqlite3'
with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
    rows = db.execute("SELECT kind,value FROM state WHERE kind IN ('watchman-device-v1','pc-mconfig-v1')").fetchall()
assert len(rows) == 2
state = {kind: json.loads(raw) for kind, raw in rows}
assert state['pc-mconfig-v1'] == pc
saved = state['watchman-device-v1']
assert saved['did'] == device[3] and saved['tid'] == behavior[2] and saved['ni'] == device[5]
assert saved['revision'] == reports and saved['expires_ms'] > saved['created_ms']
inputs.append(dict(path='state/core.sqlite3', sha256=hashlib.sha256(database.read_bytes()).hexdigest()))
result = dict(ok=True, persistedConfigAndDeviceVerified=True, inputs=inputs)
with (root / 'sqlite-audit.json').open('x') as output:
    json.dump(result, output, indent=2)
    output.write('\n')
print(json.dumps(dict(ok=True, persistedConfigAndDeviceVerified=True, inputFiles=len(inputs))))
