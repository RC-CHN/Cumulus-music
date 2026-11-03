#!/usr/bin/env python3
"""Replay public synthetic Rotate3D vectors against a local, hash-pinned DLL.

Manual Linux x86_64 research only. Requires pefile and a C compiler. The
original resampler and DSP code execute with reviewed host substitutions.
"""
import argparse
import base64
import gzip
import hashlib
import json
import math
from pathlib import Path
import platform
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "core/tests/fixtures"
DLL_SHA = "c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be"
FIXTURE_SHA = "9516097e092149973897f79cd45d7d951dce8d1b64b6db88161a40f6657d1690"
TABLE_BYTES = 151848


def sha(data):
    return hashlib.sha256(data).hexdigest()


def unpack_output(output, capacity, row):
    stride = 12 + 8 * capacity
    assert len(output) == stride * len(row["valid"])
    pcm = bytearray()
    for index, offset in enumerate(range(0, len(output), stride)):
        frames, end, phase = struct.unpack_from("<IIf", output, offset)
        assert frames <= capacity and end in (0, 1) and math.isfinite(phase)
        assert (frames, bool(end), phase) == (
            row["valid"][index], row["end"][index], row["phases"][index]
        )
        for channel in range(2):
            start = offset + 12 + 4 * capacity * channel
            pcm.extend(output[start:start + 4 * frames])
    assert sha(pcm) == row["pcmSha256"]
    assert pcm == base64.b64decode(row["pcmB64"], validate=True)
    return len(pcm) // 4


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    args = parser.parse_args()
    if not __debug__:
        raise SystemExit("run this verification without Python optimization")
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("this manual oracle requires Linux x86_64")
    with args.dll.open("rb") as file:
        raw = file.read(16 * 1024 * 1024 + 1)
    if sha(raw) != DLL_SHA:
        raise SystemExit("unexpected DLL SHA256; no code executed")

    import pefile
    image = bytearray(pefile.PE(data=raw, fast_load=True).get_memory_mapped_image())
    assert len(image) == 0x3A2400
    with gzip.open(FIXTURES / "effects-spatial-official.json.gz") as file:
        tables = base64.b64decode(json.load(file)["responses_b64"], validate=True)
    encoded = (FIXTURES / "effects-rotation-soxr-official.json.gz").read_bytes()
    if sha(encoded) != FIXTURE_SHA:
        raise SystemExit("unexpected synthetic fixture SHA256; no code executed")
    proof = json.loads(gzip.decompress(encoded))
    if (
        proof["dllSha256"] != DLL_SHA
        or proof["syntheticTablesSha256"] != sha(tables)
        or len(tables) != TABLE_BYTES * 2
    ):
        raise SystemExit("unexpected synthetic response tables; no code executed")
    image[0x1E4A00:0x1E4A00 + TABLE_BYTES] = tables[:TABLE_BYTES]
    image[0x209B30:0x209B30 + TABLE_BYTES] = tables[TABLE_BYTES:]
    with tempfile.TemporaryDirectory(prefix="cumulus-rotation-soxr-") as directory:
        mapped = Path(directory) / "official.mapped"
        mapped.write_bytes(image)
        exe = Path(directory) / "oracle"
        subprocess.run(
            ["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-ffp-contract=off",
             str(Path(__file__).with_name("rotation_soxr_wrapper.c")), "-o", str(exe)],
            timeout=30, check=True,
        )
        samples = 0
        for row in proof["rows"]:
            run = subprocess.run(
                [str(exe), str(mapped), str(row["rate"])],
                input=base64.b64decode(row["inputB64"], validate=True),
                capture_output=True, timeout=8, check=True,
            )
            samples += unpack_output(run.stdout, row["capacity"], row)
    print(json.dumps({"ok": True, "cases": len(proof["rows"]), "pcmSamples": samples}))


if __name__ == "__main__":
    main()
