#!/usr/bin/env python3
"""Explicit offline original-DLL core proof; never part of automatic public CI."""
import argparse
import hashlib
import json
import platform
import struct
import subprocess
from pathlib import Path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dll', required=True, type=Path)
    parser.add_argument('--root', required=True, type=Path)
    args = parser.parse_args()
    if platform.system() != 'Linux' or platform.machine() != 'x86_64':
        parser.error('the reviewed MS-ABI core host requires Linux x86_64')
    repo = Path(__file__).resolve().parents[2]
    fixture = json.loads((repo / 'core/tests/fixtures/anonymous_identity_vectors.json').read_text())
    source = Path(__file__).with_name('native_anonymous_identity_oracle.c')
    provenance = fixture['provenance']
    dll = args.dll.resolve(strict=True)
    if sha(dll.read_bytes()) != provenance['binarySha256']:
        parser.error('the original cloudmusic.dll does not match the pinned x64 source')
    if sha(source.read_bytes()) != provenance['oracleHelperSha256']:
        parser.error('the host adapters differ from the frozen oracle provenance')
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    child = root / 'oracle'
    subprocess.run(['cc', '-O2', '-Wall', '-Wextra', '-Werror', str(source), '-o', str(child)],
                   check=True, capture_output=True, timeout=30)
    packet = bytearray(struct.pack('<I', len(fixture['cases'])))
    for case in fixture['cases']:
        data = case['deviceId'].encode('utf-8')
        packet.extend(struct.pack('<I', len(data)))
        packet.extend(data)
    run = subprocess.run([str(child), str(dll)], input=packet,
                         capture_output=True, timeout=5, check=True)
    offset = 0
    for case in fixture['cases']:
        length, = struct.unpack_from('<I', run.stdout, offset)
        offset += 4
        observed = run.stdout[offset:offset + length].decode('ascii')
        offset += length
        if observed != case['encoded']:
            raise AssertionError(f"native core differs for synthetic case {case['name']}")
    assert offset == len(run.stdout)
    report = dict(ok=True, originalCoreExecuted=True, cases=len(fixture['cases']),
                  binarySha256=provenance['binarySha256'],
                  hostAdaptersSha256=provenance['oracleHelperSha256'],
                  rawOutputSha256=sha(run.stdout), nativeDllInitialization=False,
                  cefUtf16ConversionExecuted=False, arithmeticSeamsHooked=False,
                  networkRequests=0, registrationRequests=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
