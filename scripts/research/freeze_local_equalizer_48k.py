#!/usr/bin/env python3
"""Compact the two frozen synthetic 48 kHz SDK runs; never loads an SDK."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path


ORIGINALS = (
    '4e14e0a17cd2977aafc693f0af89465232d05f82537cd945eb364aa85fcd8ca2',
    '750aa129734bffcdfc81d8d1a708e14853b049127305c059f303417465cd2095',
)


def material(plan):
    geq, tone, stereo = plan
    bass, treble = tone or (0, 0)
    presence, stereoizer = stereo or (0, 0)
    return {
        'eq': {'on': geq is not None, 'eqs': geq or [0] * 10},
        'bt': {'on': tone is not None, 'bass': bass, 'treble': treble},
        'se': {'on': stereo is not None, 'presence': presence, 'stereoizer': stereoizer,
               'sshaper': True, 'ambience': 1},
        'rvb': {'on': False},
    }


def action(row):
    if not row:
        return None
    if row['kind'] == 'original_full_old_material':
        return {'kind': 'full_apply', 'material': material(row['plan'])}
    return {'kind': {'original_outer_master_off': 'master_off',
                     'original_outer_seek_reset': 'seek_reset'}[row['kind']]}


def compact(source_paths):
    cases, inputs = [], {}
    for path, sha in zip(source_paths, ORIGINALS, strict=True):
        compressed = path.read_bytes()
        assert hashlib.sha256(compressed).hexdigest() == sha
        for original in json.loads(gzip.decompress(compressed))['cases']:
            if original['rate'] != 48_000:
                continue
            assert original['capacity'] == 2048 and original['inputFormatEnum'] == 3
            assert original['internalSeFormat'] == '0x3102' and original['originalTextUnmodified']
            assert original['originalInnerGraphExecuted'] and original['originalOuterCreateInitAndControlsExecuted']
            assert not original['originalOuterFifoExecuted']
            blocks = []
            for row in original['blocks']:
                bits = tuple(row['inputInterleavedBits'])
                assert len(bits) == row['inputFrames'] * 2
                block = {key: row[key] for key in ('sourceFirstFrame', 'inputFrames', 'inputEnd',
                                                     'outputFrames', 'outputEnd', 'outputSha256')}
                block['inputBlock'] = inputs.setdefault(bits, len(inputs))
                block['actionBefore'] = action(row['actionBefore'])
                block['outputPrefixInterleavedBits'] = row.get('outputPrefixInterleavedBits') or None
                blocks.append(block)
            assert blocks[-1]['outputEnd']
            cases.append(dict(name=original['name'], initialMaterial=material(original['initialPlan']),
                              inputFrames=original['inputFrames'], outputFrames=original['outputFrames'],
                              blocks=blocks))
    assert len({row['name'] for row in cases}) == len(cases) == 14
    assert sum(len(row['blocks']) for row in cases) == 159
    return dict(schema='cumulus-local-equalizer-full-official-48k-v1', sampleRate=48_000,
                originalOuterCreateInitAndControlsExecuted=True, originalInnerGraphExecuted=True,
                originalOuterFifoExecuted=False, cloudCefJsonExecuted=False, nativeWindows=False,
                dspReplaced=False, inputBlocks=list(inputs), cases=cases)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('joint', type=Path)
    parser.add_argument('extended', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    payload = compact((args.joint, args.extended))
    encoded = json.dumps(payload, separators=(',', ':')).encode() + b'\n'
    with args.output.open('xb') as file:
        file.write(gzip.compress(encoded, mtime=0))


if __name__ == '__main__':
    main()
