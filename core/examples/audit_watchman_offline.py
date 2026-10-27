"""Read-only, offline audit of the executable Rust Watchman proof.
The archived Python codec is an independent audit implementation, never a client
runtime dependency. No network or browser collection is performed here.
"""
import argparse
import base64
import hashlib
import importlib.util
import json
from pathlib import Path
import sqlite3
import struct

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('proof', type=Path)
args = parser.parse_args()
root = args.proof.resolve()
evidence = json.loads((root / 'evidence.json').read_text())
report = json.loads((root / 'report.json').read_text())
script = Path('references/recovered-client/scripts/netease_watchman.py')
spec = importlib.util.spec_from_file_location('archived_watchman_audit', script)
legacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(legacy)
assert evidence['sourceSha256'] == legacy.SOURCE_SHA256
assert json.loads(legacy.text_decode(evidence['encodedConfiguration'])) == evidence['configuration']
assert legacy.request_nonce(evidence['timestampMs'], bytes.fromhex(evidence['nonceRandom'])) == evidence['nonce']
codec = legacy.PacketCodec.from_file('references/netease-3.1.42.205594-analysis/protocol/watchman-remote/codec-constants.json')
payload = bytes.fromhex(evidence['payloadHex'])
assert codec.decode(evidence['packet']) == payload
assert codec.encode(payload, bytes.fromhex(evidence['packetSeed'])) == evidence['packet']
fields = {x['key']: x for x in json.loads(Path('core/src/watchman/fields.json').read_text())}
expected = bytearray()
for key, value, failed in evidence['fields']:
    f = fields[key]
    data = b''
    if not failed:
        if f['type'] == 'STRING':
            data = value.encode('utf-16-le')[:f['limit'] * 2].decode('utf-16-le').encode('utf8')
        elif f['type'] == 'NUMBER':
            data = struct.pack('>I', int(value) % 2**32)[-f['limit']:]
        elif f['type'] == 'ARRAY':
            data = b''.join(struct.pack('>I', int(v) % 2**32)[-n:] for v, n in zip(value, f['limit']))
        else:
            raise AssertionError('unexpected synthetic fixture field')
    expected.extend(struct.pack('>HH', f['id'], len(data)) + data)
assert payload == expected
connection = sqlite3.connect((root / 'state/core.sqlite3').as_uri() + '?mode=ro', uri=True)
rows = connection.execute('SELECT owner,kind,key,value FROM state').fetchall()
assert len(rows) == 1
owner, kind, key, value = rows[0]
assert (owner, kind, key) == ('device', 'watchman-device-v1', 'YD00000558929251')
state = json.loads(value)
assert state == {'revision': 1, 'did': 'synthetic-did', 'version': '2.7.5_af2952a4', 'created_ms': 1000, 'expires_ms': 72001000, 'tid': 'synthetic-tid', 'ni': 'synthetic-ni', 'clock_offset_ms': 0}
assert json.loads(legacy.text_decode(evidence['ndInfo'])) == {'r': 1, 'd': state['did'], 'i': state['ni']}
statuses = []
for trace in evidence['traces']:
    result = trace['result']
    if 'error' in result:
        assert trace['responses'] == [470]
        assert result['error'] == 'illegal-business'
        statuses.append('IllegalBusiness')
        continue
    token = json.loads(legacy.text_decode(result['token']))
    expected = {'r': 1, 'd': state['did'], 'b': evidence['nonce']}
    if trace['responses'] == []:
        expected['t'] = base64.b64encode(hashlib.md5(('1' + state['did'] + evidence['nonce'] + 'WoeTpXnDDPhiAvsJUUIY3RdAo2PKaVwi').encode()).digest()).decode()
        assert result['status'] == 'TimeoutFallback'
    elif trace['responses'] == [420, None]:
        assert result['status'] == 'UncheckedRetry'
    else:
        assert trace['responses'] == [200]
        assert result['status'] == 'BehaviorAccepted'
    assert token == expected
    statuses.append(result['status'])
assert report['networkRequests'] == 0 and report['sdkProviderConnected'] is False
summary = dict(ok=True, independentArchivedPythonCodec=True, packetAndFieldBytes=True, nativeSqliteReopen=True,
               tokenStatuses=statuses, networkRequests=0, serverAcceptanceTested=False)
print(json.dumps(summary))
