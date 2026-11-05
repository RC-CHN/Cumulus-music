#!/usr/bin/env python3
"""Manually freeze synthetic stereo FLAC using public libFLAC 1.5.0.

No account, original DLL, external service or audio device is used. Public CI
consumes the frozen FLAC/manifest and never needs this generator or encoder.
"""
import argparse
import ctypes as c
import hashlib
import json
from pathlib import Path


RATE = 44100
FRAMES = RATE * 2 + 7
BLOCK = 1024


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    library = args.library.resolve(strict=True)
    output = args.output.resolve()
    manifest = output.with_suffix('.manifest.json')
    assert not output.exists() and not manifest.exists()
    sdk = c.CDLL(str(library))
    assert c.c_char_p.in_dll(sdk, 'FLAC__VERSION_STRING').value == b'1.5.0'

    def fn(name, result, *params):
        function = getattr(sdk, name)
        function.restype = result
        function.argtypes = params
        return function

    pointer, u32, u64 = c.c_void_p, c.c_uint32, c.c_uint64
    create = fn('FLAC__stream_encoder_new', pointer)
    delete = fn('FLAC__stream_encoder_delete', None, pointer)
    new_metadata = fn('FLAC__metadata_object_new', pointer, c.c_int)
    delete_metadata = fn('FLAC__metadata_object_delete', None, pointer)
    append = fn('FLAC__metadata_object_seektable_template_append_spaced_points_by_samples', c.c_int, pointer, u32, u64)
    sort = fn('FLAC__metadata_object_seektable_template_sort', c.c_int, pointer, c.c_int)
    table = new_metadata(3)
    encoder = create()
    assert table and encoder
    try:
        assert append(table, BLOCK, FRAMES) and sort(table, 1)
        metadata = (pointer * 1)(table)
        assert fn('FLAC__stream_encoder_set_metadata', c.c_int, pointer, c.POINTER(pointer), u32)(encoder, metadata, 1)
        for name, value in [('channels', 2), ('bits_per_sample', 16), ('sample_rate', RATE), ('blocksize', BLOCK), ('compression_level', 5)]:
            assert fn('FLAC__stream_encoder_set_' + name, c.c_int, pointer, u32)(encoder, value)
        assert fn('FLAC__stream_encoder_set_total_samples_estimate', c.c_int, pointer, u64)(encoder, FRAMES)
        output.parent.mkdir(parents=True, exist_ok=True)
        assert fn('FLAC__stream_encoder_init_file', c.c_int, pointer, c.c_char_p, pointer, pointer)(encoder, str(output).encode(), None, None) == 0
        process = fn('FLAC__stream_encoder_process_interleaved', c.c_int, pointer, c.POINTER(c.c_int32), u32)
        digest = hashlib.sha256()
        for start in range(0, FRAMES, BLOCK):
            count = min(BLOCK, FRAMES - start)
            pcm = []
            for frame in range(start, start + count):
                left = ((frame * 97 + (frame // 128) * 23) % 8192) - 4096
                pcm.extend((left, -(abs(left) // 8) if left >= 0 else abs(left) // 8))
            import struct
            digest.update(struct.pack('<' + 'h' * len(pcm), *pcm))
            assert process(encoder, (c.c_int32 * len(pcm))(*pcm), count)
        assert fn('FLAC__stream_encoder_finish', c.c_int, pointer)(encoder)
    finally:
        delete(encoder)
        delete_metadata(table)
    data = output.read_bytes()
    value = {
        'schema': 'cumulus-stereo-pcm-source-v1', 'synthetic': True,
        'sourceRate': RATE, 'sourceChannels': 2, 'bitsPerSample': 16,
        'maxBlockFrames': BLOCK, 'sourceFrames': FRAMES,
        'seekSpacingFrames': BLOCK, 'libFLAC': '1.5.0',
        'pcmPacking': 'interleaved signed16 little-endian',
        'pcmSha256': digest.hexdigest(), 'flacSha256': hashlib.sha256(data).hexdigest(),
        'generatorSha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'bytes': len(data), 'accountOrOriginalMedia': False,
    }
    manifest.write_text(json.dumps(value, indent=2) + '\n')
    print(json.dumps(value))


if __name__ == '__main__':
    main()
