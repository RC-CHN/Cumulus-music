#!/usr/bin/env python3
"""Manual full-SDK synthetic I/O replay and refreeze; never a CI/native product path.

The original DLL is a separately supplied, hash-pinned local input. No original
machine code, account material or parameter snapshots are distributed here.
Inputs come from frozen synthetic IEEE754 bits to avoid host-libm drift.
"""
import argparse
import gzip
import hashlib
import json
import math
import os
import platform
import socket
import struct
import subprocess
import sys
from pathlib import Path

FIXTURES = {
    'effects-local-stereo-slider-official.json.gz': (
        '84f9fe4aa35cb8208ff887bac8c050e7d80f2e9d8f9c04628ae065c797072783',
        'cumulus-full-local-stereo-slider-official-v1', 2, 60),
    'effects-local-stereo-after-eof-reset-official.json.gz': (
        '669f4cefcd1ea28eebf2d0984821e1637646d52a494a760fa83a2c9a9ef1c737',
        'cumulus-full-local-stereo-after-eof-reset-official-v1', 2, 50),
    'effects-local-stereo-full-official.json.gz': (
        '6a78ec2e80faa45ef9fa91228fcbf9c8e794ac838e9e1b2a089ed1b304930ed3',
        'cumulus-full-local-stereo-official-v1', 24, 1002),
    'effects-local-stereo-first-eof-official.json.gz': (
        'be546e54b7e14c2c21c96faa8e31004dfed211f9f5ac9c1f64920848ccf73aa7',
        'cumulus-full-local-stereo-first-eof-v1', 4, 92),
}
SLOTS = [hex(n) for n in [8, 0x18, 0x58, 0x48, 0x50, 0x70, 0x68,
                         0x60, 0xA0, 0x90, 0x98, 0x28, 0x30, 0x40,
                         0x78, 0x20, 0xA8, 0x38, 0xB0, 0x10]]
NAMES = {8: 'input_converter', 0x10: 'output_converter', 0x18: 'loudness',
         0x20: 'RVB', 0x28: 'SE', 0x30: 'Rotate', 0x48: 'GEQ',
         0x50: 'BT', 0x70: 'extra_PEQ', 0xB0: 'B0'}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def bits(raw):
    return list(struct.unpack('<%dI' % (len(raw) // 4), raw))


def check(ok, message):
    if not ok:
        raise ValueError(message)


def groups(obj):
    return obj['phases'] if 'phases' in obj else obj['cases']


def load_fixture(path):
    path = Path(path)
    check(path.name in FIXTURES, 'unknown fixture name')
    digest, schema, case_count, block_count = FIXTURES[path.name]
    check(path.is_file() and path.stat().st_size < 8 * 1024 * 1024,
          'expected bounded local gzip fixture')
    raw = path.read_bytes()
    check(sha(raw) == digest, 'frozen fixture SHA256 mismatch')
    import io
    with gzip.GzipFile(fileobj=io.BytesIO(raw)) as src:
        decoded = src.read(16 * 1024 * 1024 + 1)
    check(len(decoded) <= 16 * 1024 * 1024, 'oversized decompressed fixture')
    obj = json.loads(decoded)
    check(obj['schema'] == schema and obj['fullOriginalSdkGraph'] is True,
          'unexpected fixture schema')
    check(obj['realWindowsExecution'] is False and obj['originalOuterFifo'] is False,
          'unexpected fixture execution claim')
    check(obj['dspReplaced'] is False, 'fixture replaced DSP')
    check(len(groups(obj)) == case_count, 'unexpected case count')
    check(sum(len(c['blocks']) for c in groups(obj)) == block_count,
          'unexpected block count')
    identities = set()
    for case in groups(obj):
        rate, cap = case['rate'], case['capacity']
        identity = (case['name'], rate, cap)
        check(identity not in identities, 'duplicate case')
        identities.add(identity)
        check(rate in [44100, 48000, 96000] and cap in [512, 2048], 'unsupported geometry')
        check(case['inputFormatEnum'] == 3 and case['internalSeFormat'] == '0x3102',
              'unexpected original format')
        check(case['originalTextUnmodified'] is True and case['actualNlsReplyCount'] == 0,
              'unexpected source/locale claim')
        check(case['crtLocaleRequests'] in [[], ['C', 'C']], 'unexpected locale requests')
        check(0 < len(case['blocks']) <= math.ceil(rate / cap) + 16, 'unbounded case')
        check(sum(b['inputFrames'] for b in case['blocks']) == case['inputFrames'], 'input total')
        check(sum(b['outputFrames'] for b in case['blocks']) == case['outputFrames'], 'output total')
        for block in case['blocks']:
            n = block['inputFrames']
            check(type(n) is int and 0 <= n <= cap, 'input geometry')
            check(type(block['inputEnd']) is bool, 'input EOF type')
            values = block['inputInterleavedBits']
            check(len(values) == 2 * n and all(type(v) is int and 0 <= v <= 0xFFFFFFFF for v in values),
                  'input bit geometry')
            raw_input = struct.pack('<%dI' % len(values), *values)
            check(all(math.isfinite(v) for v in struct.unpack('<%df' % len(values), raw_input)),
                  'nonfinite synthetic input')
            check(0 <= block['outputFrames'] <= cap, 'output geometry')
            check(block['childOrder'] == [c['slot'] for c in block['children']], 'child identity order')
            check(len(block['childOrder']) == len(set(block['childOrder'])), 'duplicate child')
            check(block['childOrder'] == [s for s in SLOTS if s in block['childOrder']], 'child source order')
            actions = block['actionBefore']
            actions = actions if isinstance(actions, list) else ([] if actions is None else [actions])
            check(all(a['kind'] in ['original_se_switch_off', 'original_graph_seek_reset',
                  'original_se_controls', 'original_se_enable',
                  'original_graph_seek_reset_after_complete_eof'] for a in actions), 'unexpected action')
    return obj


def isolated():
    # A separate Linux user/network namespace is required even though this
    # harness contains no network service adapters and never loads DLL imports.
    return {name for _, name in socket.if_nameindex()} <= {'lo'}


def require_isolation():
    check(platform.system() == 'Linux' and platform.machine() in ['x86_64', 'AMD64'],
          'manual interpreter host must be Linux x86_64')
    check(__debug__, 'do not run this oracle with Python -O')
    if not isolated():
        result = subprocess.run(['unshare', '-Urn', sys.executable, str(Path(__file__).resolve()),
                                 *sys.argv[1:]], check=False)
        raise SystemExit(result.returncode)


def equal(actual, expected, where):
    # Include only field path in failures; never dump original process memory.
    if type(actual) is not type(expected):
        raise ValueError('type mismatch at ' + where)
    if isinstance(expected, dict):
        check(actual.keys() == expected.keys(), 'keys mismatch at ' + where)
        for key in expected:
            equal(actual[key], expected[key], where + '/' + key)
    elif isinstance(expected, list):
        check(len(actual) == len(expected), 'length mismatch at ' + where)
        for i, (a, e) in enumerate(zip(actual, expected)):
            equal(a, e, where + '/' + str(i))
    else:
        check(actual == expected, 'original output mismatch at ' + where)


def replay_case(dll, expected, shared=None):
    from full_stereo_oracle.vm import VM, BASE
    from unicorn.x86_const import (UC_X86_REG_RCX, UC_X86_REG_RDX,
        UC_X86_REG_RSP, UC_X86_REG_RIP, UC_X86_REG_XMM1, UC_X86_REG_XMM2, UC_X86_REG_XMM3)
    rate, cap, name = expected['rate'], expected['capacity'], expected['name']
    shared = {} if shared is None else shared
    if 'vm' not in shared:
        vm = VM(dll)
        obj = vm.allocate(0x140)
        for entry in [0x1C04E0, 0x19F208, 0x1A0508]:
            check(vm.run(entry, 0) & 255 == 1, 'original CRT initialization failed')
        vm.run(0x1A050, obj)
        check(vm.run(0x1AEC0, obj, 3, cap, 2, rate) == 1, 'original SDK initialization failed')
        vm.run(0x1BBA0, obj)
        vm.run(0x1C590, obj, 0)
        params = vm.allocate(24)
        vm.u.mem_write(params, struct.pack('<ffffB3xf', 0., .005, .1, 0., 1, 4.))
        vm.run(0x1D410, obj, params)
        vm.run(0x1D360, obj, 1)
        if name not in ['default_all_off', 'se_off_on_hot_off_on_both_pending_reset']:
            vm.u.reg_write(UC_X86_REG_XMM1, struct.unpack('<I', struct.pack('<f', 3.))[0])
            vm.u.reg_write(UC_X86_REG_XMM2, struct.unpack('<I', struct.pack('<f', 5.))[0])
            vm.run(0x1DDC0, obj, 0, 0, 1, 1)
            vm.run(0x1DD00, obj, 1)
        pcm, np, ep = vm.allocate(cap * 8), vm.allocate(4), vm.allocate(1)
        shared.update(vm=vm, obj=obj, pcm=pcm, np=np, ep=ep)
    else:
        vm, obj, pcm, np, ep = (shared[k] for k in ['vm', 'obj', 'pcm', 'np', 'ep'])
    top = {vm.q(obj + off): off for off in range(8, 0xB8, 8) if vm.q(obj + off)}
    pending, rows, mixes = {}, [], []
    capture = False
    active = False
    block_index = -1

    def flow():
        return vm.i(np), bool(vm.u.mem_read(ep, 1)[0])

    def begin():
        if not active:
            return
        node = vm.reg(UC_X86_REG_RCX)
        if node not in top:
            return
        slot = top[node]
        n, end = flow()
        ret = vm.q(vm.reg(UC_X86_REG_RSP))
        check(ret not in pending, 'nested original top-level callback')
        row = {'slot': hex(slot), 'name': NAMES.get(slot, 'original_other'),
               'processRva': hex(vm.im.slot(vm.q(node) - BASE, 0x10)),
               'inputFrames': n, 'inputEnd': end}
        if slot in [0x20, 0x28, 0x48, 0x50]:
            row['bypassBefore48_49'] = list(vm.u.mem_read(node + 0x48, 2))
        pending[ret] = (row, node)

    def finish():
        if not active:
            return
        ret = vm.reg(UC_X86_REG_RIP)
        if ret not in pending:
            return
        row, node = pending.pop(ret)
        n, end = flow()
        check(n <= cap, 'original child exceeded capacity')
        work = vm.q(obj + 0x118)
        raw = bytes(vm.u.mem_read(work, cap * 8))
        valid = raw[:n * 4] + raw[cap * 4:cap * 4 + n * 4]
        check(all(math.isfinite(v) for v in struct.unpack('<%df' % (2 * n), valid)),
              'nonfinite original child output')
        row.update(outputFrames=n, outputEnd=end, planarPcmSha256=sha(valid))
        if capture:
            count = min(64, n)
            row.update(planarPrefixFrames=count,
                       planarPrefixBits=[bits(valid[:count * 4]), bits(valid[n * 4:n * 4 + count * 4])])
        if int(row['slot'], 16) in [0x20, 0x28, 0x48, 0x50]:
            row['bypassAfter48_49'] = list(vm.u.mem_read(node + 0x48, 2))
        rows.append(row)

    def before_mix():
        if block_index != 0 or len(pending) != 1 or 'firstBlockFullCapacityPreblend' not in expected:
            return
        row, node = next(iter(pending.values()))
        slot = int(row['slot'], 16)
        if slot not in [0x68, 0x78, 0x20]:
            return
        sp = vm.reg(UC_X86_REG_RSP)
        ret = vm.q(sp) - BASE
        entry = vm.im.slot(vm.q(node) - BASE, 0x10)
        if not entry <= ret < entry + 0x400:
            return
        a, b = vm.reg(UC_X86_REG_RCX), vm.reg(UC_X86_REG_RDX)
        n, work = vm.i(sp + 0x38), vm.q(obj + 0x118)
        if a not in [work, work + cap * 4] or n != cap:
            return
        wet, dry = bytes(vm.u.mem_read(a, n * 4)), bytes(vm.u.mem_read(b, n * 4))
        gains = [struct.unpack('<f', struct.pack('<I', vm.reg(r) & 0xFFFFFFFF))[0]
                 for r in [UC_X86_REG_XMM2, UC_X86_REG_XMM3]]
        gains += [struct.unpack('<f', vm.u.mem_read(sp + o, 4))[0] for o in [0x28, 0x30]]
        mixes.append({'slot': hex(slot), 'channel': 0 if a == work else 1,
                      'actualValidFramesAtMix': vm.i(np), 'mixedCapacity': n, 'gains': gains,
                      'wetEqualsDry': wet == dry, 'wetSha256': sha(wet), 'drySha256': sha(dry),
                      'first16WetBits': bits(wet)[:16], 'first16DryBits': bits(dry)[:16]})

    for entry in {vm.im.slot(vm.q(node) - BASE, 0x10) for node in top}:
        vm.hook(BASE + entry, begin)
    for site in vm.im.graph_return_sites():
        vm.hook(site, finish)
    vm.hook(BASE + 0xA9C50, before_mix)
    actual_blocks = []
    cursor = 0
    for i, wanted in enumerate(expected['blocks']):
        action = wanted['actionBefore']
        actions = action if isinstance(action, list) else ([] if action is None else [action])
        for event in actions:
            kind = event['kind']
            if kind == 'original_se_switch_off':
                check(i == 2 and name == 'on_to_off', 'wrong off scenario')
                vm.run(0x1DD00, obj, 0)
            elif kind == 'original_se_enable':
                check(type(event['enabled']) is bool, 'invalid enable flag')
                vm.run(0x1DD00, obj, int(event['enabled']))
            elif kind == 'original_se_controls':
                check((event['presence'], event['stereoizer']) in [(3.,5.), (6.,8.), (2.,4.)]
                      and event['shaper'] is True and event['ambience'] == 1, 'invalid synthetic controls')
                vm.u.reg_write(UC_X86_REG_XMM1, struct.unpack('<I', struct.pack('<f', event['presence']))[0])
                vm.u.reg_write(UC_X86_REG_XMM2, struct.unpack('<I', struct.pack('<f', event['stereoizer']))[0])
                vm.run(0x1DDC0, obj, 0, 0, 1, 1)
            else:
                check(event['newSyntheticSourceFrame'] in [8000,16000], 'invalid reset cursor')
                if 'seFlagsBeforeReset' in event:
                    equal(list(vm.u.mem_read(vm.q(obj + 0x28) + 0x48, 2)), event['seFlagsBeforeReset'], 'beforeResetFlags')
                vm.run(0x1B960, obj)
                cursor = event['newSyntheticSourceFrame']
                if 'seFlagsAfterReset' in event:
                    equal(list(vm.u.mem_read(vm.q(obj + 0x28) + 0x48, 2)), event['seFlagsAfterReset'], 'afterResetFlags')
        check(wanted['sourceFirstFrame'] == cursor, 'source cursor mismatch')
        n = wanted['inputFrames']
        check(i == len(expected['blocks']) - 1 or not wanted['outputEnd'], 'premature expected end')
        raw = struct.pack('<%dI' % (2 * n), *wanted['inputInterleavedBits']) + bytes((cap - n) * 8)
        vm.u.mem_write(pcm, raw)
        vm.puti(np, n)
        vm.u.mem_write(ep, bytes([wanted['inputEnd']]))
        check(not pending, 'unfinished previous child')
        rows.clear()
        block_index, capture, active = i, any('planarPrefixBits' in c for c in wanted['children']), True
        vm.run(0x1AA50, obj, pcm, np, ep)
        active = False
        check(not pending, 'unfinished original graph child')
        got, end = flow()
        check(got <= cap, 'original graph exceeded capacity')
        out = bytes(vm.u.mem_read(pcm, got * 8))
        check(all(math.isfinite(v) for v in struct.unpack('<%df' % (got * 2), out)),
              'nonfinite original final output')
        row = {'sourceFirstFrame': cursor, 'inputFrames': n, 'inputEnd': wanted['inputEnd'],
               'actionBefore': action, 'outputFrames': got, 'outputEnd': end,
               'outputSha256': sha(out), 'childOrder': [c['slot'] for c in rows],
               'inputInterleavedBits': bits(raw[:n * 8]), 'children': list(rows)}
        if 'outputPrefixFrames' in wanted:
            count = min(64, got)
            row.update(outputPrefixFrames=count, outputPrefixInterleavedBits=bits(out[:count * 8]))
        equal(row, wanted, '%s/%d/%d/block%d' % (name, rate, cap, i))
        actual_blocks.append(row)
        cursor += n
    check(actual_blocks[-1]['outputEnd'], 'graph did not drain')
    vm.puti(np, 0)
    vm.u.mem_write(ep, b'\1')
    vm.run(0x1AA50, obj, pcm, np, ep)
    check(flow() == (0, True), 'original post-EOF graph changed')
    check(bytes(vm.u.mem_read(BASE + 0x1000, 0x1E2000)) == vm.im.data[0x1000:0x1E3000],
          'original text modified')
    check(not any('NLS' in e for e in vm.import_events), 'unexpected NLS host fallback used')
    actual = {'name': name, 'rate': rate, 'capacity': cap, 'inputFormatEnum': 3,
              'internalSeFormat': hex(vm.i(vm.q(obj + 0x28) + 0x20)),
              'inputFrames': sum(b['inputFrames'] for b in actual_blocks),
              'outputFrames': sum(b['outputFrames'] for b in actual_blocks),
              'originalTextUnmodified': True, 'finalSeFlags': list(vm.u.mem_read(vm.q(obj + 0x28) + 0x48, 2)),
              'blocks': actual_blocks, 'actualNlsReplyCount': 0,
              'crtLocaleRequests': [e['CRTLocaleRequest'] for e in vm.import_events if 'CRTLocaleRequest' in e]}
    if 'firstBlockFullCapacityPreblend' in expected:
        actual['firstBlockFullCapacityPreblend'] = mixes
    equal(actual, expected, '%s/%d/%d' % (name, rate, cap))
    return actual


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', action='append', type=Path, required=True,
                        help='one or more hash-pinned public synthetic corpus gzip files')
    parser.add_argument('--audit-only', action='store_true', help='audit corpus without native dependencies/execution')
    parser.add_argument('--dll', type=Path, help='caller-owned exact original SDK DLL, never distributed')
    parser.add_argument('--output-dir', type=Path, help='new private output directory; must not exist')
    args = parser.parse_args()
    check(len(args.fixture) <= 4 and len(set(args.fixture)) == len(args.fixture), 'duplicate/too many fixtures')
    fixtures = [(path, load_fixture(path)) for path in args.fixture]
    if args.audit_only:
        print(json.dumps({'auditOnly': True, 'cases': sum(len(groups(o)) for _, o in fixtures),
                          'blocks': sum(len(c['blocks']) for _, o in fixtures for c in groups(o))}))
        return
    check(args.dll is not None and args.output_dir is not None, '--dll and --output-dir are required for replay')
    require_isolation()
    check(isolated(), 'network isolation was not established')
    check(not args.output_dir.exists(), 'output directory already exists')
    from full_stereo_oracle.image import NativeImage, DLL_SHA256
    NativeImage(args.dll)  # hash/layout validation before any original instruction
    args.output_dir.mkdir(parents=True)
    receipts = []
    for path, frozen in fixtures:
        actual = dict(frozen)
        group_key = 'phases' if 'phases' in frozen else 'cases'
        actual[group_key] = []
        shared = {} if frozen.get('sameOriginalInstance') else None
        for case in groups(frozen):
            actual[group_key].append(replay_case(args.dll, case, shared))
            print('CASE_PASS', json.dumps({k: case[k] for k in ['name', 'rate', 'capacity']}), flush=True)
        encoded = json.dumps(actual, separators=(',', ':')).encode() + b'\n'
        output = args.output_dir / path.name
        with output.open('xb') as out:
            out.write(gzip.compress(encoded, mtime=0))
        receipts.append({'fixture': path.name, 'frozenSha256': sha(path.read_bytes()),
                         'outputSha256': sha(output.read_bytes()), 'semanticComparison': 'exact',
                         'cases': len(groups(actual)), 'blocks': sum(len(c['blocks']) for c in groups(actual))})
    scripts = [Path(__file__), *sorted((Path(__file__).parent / 'full_stereo_oracle').glob('*.py'))]
    manifest = {'schema': 'cumulus-full-stereo-manual-replay-v1', 'dllSha256': DLL_SHA256,
                'fullOriginalSdkGraph': True, 'realWindowsExecution': False, 'originalOuterFifo': False,
                'originalTextPatched': False, 'dspReplaced': False, 'networkNamespaceIsolated': True,
                'input': 'frozen synthetic exact f32 bits, no host libm waveform generation',
                'host': 'Unicorn x64; bounded declared single-thread Windows/CRT host services; not Windows',
                'sourceSha256': {p.name: sha(p.read_bytes()) for p in scripts}, 'receipts': receipts}
    with (args.output_dir / 'manual-replay-manifest.json').open('x') as out:
        out.write(json.dumps(manifest, indent=2) + '\n')
    print('REPLAY_PASS', json.dumps(receipts), flush=True)


if __name__ == '__main__':
    main()
