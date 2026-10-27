"""Verification only: freeze vectors from unmodified pinned official SDK classes.

No network, account credentials, or archived client implementation is executed.
Requires crcmod 1.7 in the optional verification environment, not in Rust runtime.
"""
import ast
import base64
import hashlib
import hmac
import json
import logging
from pathlib import Path
import random
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'references/oss-sdk-oracle'
OUTPUT = ROOT / 'core/tests/fixtures'
PINNED = {
    'auth.py': '3d8851416c8a61c1f656377ddfded6c4286443ebae7164bd2f069fc83ec43ee3',
    'utils.py': 'e3a59eb304af0e486c08bae9be29ce6c21c4ff2e1ed5dfb38b5462f9a2cfc420',
}
REVISION = 'eb329ec91d24e2e52646f244863232342fa05161'

def original(filename, names, namespace):
    raw = (SOURCE / filename).read_bytes()
    assert hashlib.sha256(raw).hexdigest() == PINNED[filename]
    nodes = [n for n in ast.parse(raw).body if isinstance(n, ast.ClassDef) and n.name in names]
    assert {n.name for n in nodes} == set(names)
    exec(compile(ast.Module(body=nodes, type_ignores=[]), filename, 'exec'), namespace)
    return namespace

class Headers(dict):
    def __init__(self, values):
        super().__init__((k.lower(), v) for k, v in values.items())

def main():
    date = 'Sun, 22 Nov 2015 08:16:38 GMT'
    ns = original('auth.py', ['AuthBase', 'ProviderAuth'], {
        'hmac': hmac, 'hashlib': hashlib, 'is_py2': False,
        'to_bytes': lambda v: v.encode('utf-8') if isinstance(v, str) else v,
        'OSS_SECURITY_TOKEN': 'x-oss-security-token', 'logger': logging.getLogger('oss-fixture'),
        'utils': SimpleNamespace(http_date=lambda: date,
            b64encode_as_string=lambda v: base64.b64encode(v).decode('ascii')),
    })
    credentials = SimpleNamespace(get_access_key_id=lambda: 'oracle-key',
        get_access_key_secret=lambda: 'oracle-secret', get_security_token=lambda: 'oracle-sts-token')
    provider = ns['ProviderAuth'](SimpleNamespace(get_credentials=lambda: credentials))
    cases = []
    for method, params in [
        ('POST', {'uploads': ''}), ('PUT', {}),
        ('PUT', {'partNumber': '2', 'uploadId': 'id+with/=special'}),
        ('GET', {'uploadId': 'U', 'max-parts': '1000', 'part-number-marker': '9'}),
        ('POST', {'uploadId': 'U'}), ('HEAD', {}), ('GET', {}), ('DELETE', {'uploadId': 'U'}),
    ]:
        for key in ('simple.mp3', '音乐/曲目 +%.flac', '目录//emoji-😀.mp3'):
            headers = Headers({'Content-Type': 'application/octet-stream',
                'Content-MD5': base64.b64encode(hashlib.md5(b'fixture').digest()).decode('ascii'),
                'X-Oss-Meta-Z': 'last', 'x-oss-meta-a': 'first', 'Date': date})
            request = SimpleNamespace(method=method, params=params, headers=headers)
            provider._sign_request(request, 'oracle-bucket', key)
            expected = headers.pop('authorization')
            cases.append(dict(method=method, key=key, bucket='oracle-bucket', secret='oracle-secret',
                access='oracle-key', headers=dict(headers), params=params, authorization=expected))
    request = SimpleNamespace(method='GET', params={}, headers=Headers({
        'Date': date, 'x-oss-date': 'Wed, 28 Dec 2022 10:27:41 GMT'}))
    provider._sign_request(request, 'oracle-bucket', 'date-override')
    expected = request.headers.pop('authorization')
    cases.append(dict(method='GET', key='date-override', bucket='oracle-bucket', secret='oracle-secret',
        access='oracle-key', headers=dict(request.headers), params={}, authorization=expected))
    (OUTPUT / 'oss-signatures.json').write_text(json.dumps(dict(revision=REVISION, source_sha256=PINNED, cases=cases), ensure_ascii=False, indent=2)+'\n')

    import crcmod
    ns = original('utils.py', ['Crc64'], {'crcmod': crcmod, 'mkCombineFun': lambda *a, **k: None})
    rng = random.Random(164205594)
    combined = bytearray()
    crcs = []
    for length in (0, 1, 7, 9, 255, 256, 1024, 65537, 102400, 102401, 1048576):
        data = rng.randbytes(length)
        oracle = ns['Crc64']()
        oracle(data)
        split = length // 3
        prefix = ns['Crc64']()
        prefix(data[:split])
        resumed = ns['Crc64'](init_crc=prefix.crc)
        resumed(data[split:])
        assert resumed.crc == oracle.crc
        crcs.append(dict(offset=len(combined), length=length, split=split,
            prefix=str(prefix.crc), expected=str(oracle.crc)))
        combined.extend(data)
    (OUTPUT / 'oss-crc64.bin').write_bytes(combined)
    (OUTPUT / 'oss-crc64.json').write_text(json.dumps(dict(revision=REVISION,
        data_sha256=hashlib.sha256(combined).hexdigest(), cases=crcs), indent=2)+'\n')
    print(json.dumps(dict(signature_cases=len(cases), crc_cases=len(crcs), network_used=False)))

if __name__ == '__main__':
    main()
