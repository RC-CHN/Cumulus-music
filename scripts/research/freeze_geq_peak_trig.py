#!/usr/bin/env python3
"""Manual, reviewed x64 GEQ static-SSE math oracle; never run by public CI.

Needs Linux x86_64, pefile and the hash-pinned official DLL supplied separately.
Only finite nonnegative phases <=0.9*pi enter 14E580/14EB80. Neither routine
calls another function in this domain; no DLL startup/import/CRT/OS adapter or
text patch is used. This does not validate the original AVX/FMA dispatch.
"""
import argparse
import ctypes
import gzip
import hashlib
import json
import mmap
import platform
import random
import struct
from pathlib import Path

DLL_SHA256 = "c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be"


def bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def value(raw):
    return struct.unpack("<f", struct.pack("<I", raw))[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("reviewed native oracle requires Linux x86_64")
    if args.output.exists():
        raise SystemExit("output must be a new file")
    raw = args.dll.read_bytes()
    if hashlib.sha256(raw).hexdigest() != DLL_SHA256:
        raise SystemExit("official DLL hash mismatch")
    import pefile

    with pefile.PE(data=raw) as pe:
        if pe.OPTIONAL_HEADER.ImageBase != 0x180000000:
            raise SystemExit("unexpected original image base")
        image = pe.get_memory_mapped_image()
    mapped = mmap.mmap(-1, len(image), prot=mmap.PROT_READ | mmap.PROT_WRITE | mmap.PROT_EXEC)
    mapped.write(image)
    owner = ctypes.c_char.from_buffer(mapped)
    base = ctypes.addressof(owner)
    native_cos = ctypes.CFUNCTYPE(ctypes.c_float, ctypes.c_float)(base + 0x14E580)
    native_sin = ctypes.CFUNCTYPE(ctypes.c_float, ctypes.c_float)(base + 0x14EB80)
    phases = {0, 1, 0x7FFFFF, 0x800000, 0x39000000, 0x3C000000, 0x3F490FDB, 0x3FC90FDB, 0x4034F4A9}
    for raw_phase in list(phases):
        phases.update(x for x in range(max(0, raw_phase - 8), raw_phase + 9) if value(x) <= 2.8274336)
    rng = random.Random(0x14E580)
    for _ in range(10000):
        phases.add(bits(rng.uniform(0, 2.8274333)))
    for rate in (8000, 44100, 48000, 96000, 384000):
        for band in range(10):
            phase = ctypes.c_float(ctypes.c_float(31.25 * 2**band).value * ctypes.c_float(6.2831854820251465).value).value / rate
            phases.add(bits(phase))
    rows = []
    for raw_phase in sorted(phases):
        phase = value(raw_phase)
        if phase > 2.8274336:
            continue
        rows.append({"phaseBits": raw_phase, "sineBits": bits(native_sin(phase)), "cosineBits": bits(native_cos(phase))})
    corpus = {"schema": "cumulus-original-geq-static-sse-trig-v1", "dllSha256": DLL_SHA256,
              "entries": ["14E580", "14EB80"], "originalTextPatched": False,
              "osOrCrtAdapterCalls": 0, "avxDispatchTested": False, "vectors": rows}
    encoded = gzip.compress(json.dumps(corpus, separators=(",", ":")).encode() + b"\n", mtime=0)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("xb") as output:
        output.write(encoded)
    print(json.dumps({"count": len(rows), "sha256": hashlib.sha256(encoded).hexdigest()}))


if __name__ == "__main__":
    main()
