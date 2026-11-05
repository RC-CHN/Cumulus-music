#!/usr/bin/env python3
"""Manually freeze synthetic PCM from the pinned original ambience=0 node.

Requires the locally extracted official DLL, a C compiler and Linux x86_64
user/network namespaces. Never invoked by public CI or the core at runtime.
"""
import argparse
import gzip
import hashlib
import json
import math
from pathlib import Path
import platform
import random
import struct
import subprocess
import tempfile

DLL_SHA = "c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be"
FLAGS = ["-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-ffp-contract=off", "-fno-fast-math"]

def digest(data):
    return hashlib.sha256(data).hexdigest()

def mapped_image(raw):
    if digest(raw) != DLL_SHA:
        raise ValueError("unexpected official DLL hash")
    nt = struct.unpack_from("<I", raw, 0x3C)[0]
    if raw[nt:nt + 4] != b"PE\0\0":
        raise ValueError("invalid PE header")
    count = struct.unpack_from("<H", raw, nt + 6)[0]
    optional = struct.unpack_from("<H", raw, nt + 20)[0]
    table = nt + 24 + optional
    sections = [struct.unpack_from("<8sIIII", raw, table + i * 40) for i in range(count)]
    image = bytearray(max(va + size for _, _, va, size, _ in sections))
    header_size = struct.unpack_from("<I", raw, nt + 24 + 60)[0]
    image[:header_size] = raw[:header_size]
    for _, _, va, size, offset in sections:
        image[va:va + size] = raw[offset:offset + size]
    if len(image) != 0x3A2400:
        raise ValueError("unexpected mapped image size")
    return image

def bits(values):
    return list(struct.unpack("<%dI" % len(values), struct.pack("<%df" % len(values), *values)))

def plans():
    cases = [(rate, cap, shaper, ftz, "music") for rate in (44100, 48000, 96000)
             for cap in (4, 64, 512) for shaper in (False, True) for ftz in (0, 1)]
    cases += [(48000, 4096, True, 0, "music"), (8000, 8, True, 0, "music"),
              (384000, 8, True, 0, "music"), (48000, 8, True, 0, "subnormal"),
              (48000, 8, True, 1, "subnormal"), (44100, 8, True, 0, "silence")]
    for rate, cap, shaper, ftz, kind in cases:
        rng = random.Random(rate + cap + int(shaper))
        controls = [(cap, False, False, 0., 0., shaper, False),
                    (cap - 1, False, False, 5., 2.5, True, False),
                    (1, False, False, -10., 10., False, False),
                    (min(7, cap), False, False, -3., 5., True, False),
                    (min(3, cap), False, False, 10., 0., True, True),
                    (cap, True, False, 0., 5., True, False),
                    (cap - 1, True, False, 0., 5., True, False),
                    (min(7, cap), False, False, 3., 5., True, False),
                    (cap, False, True, 3., 5., True, False)]
        blocks = []
        for index, (n, bypass, end, presence, stereoizer, shaping, reset) in enumerate(controls):
            pcm = [math.sin((index * cap + i) * .127) * .3 + rng.uniform(-.01, .01) for i in range(cap)]
            pcm += [math.cos((index * cap + i) * .037) * .17 + rng.uniform(-.02, .02) for i in range(cap)]
            if kind == "silence":
                pcm = [0.] * (cap * 2)
            elif kind == "subnormal":
                pcm = [(-1 if i % 2 else 1) * (1e-40 if i % 3 else 2e-38) for i in range(cap * 2)]
            blocks.append(dict(frames=n, bypass=bypass, end=end, presence=presence,
                               stereoizer=stereoizer, shaper=shaping, reset=reset, inputBits=bits(pcm)))
        yield dict(name=f"{rate}-{cap}-{int(shaper)}-{ftz}-{kind}", rate=rate,
                   capacity=cap, callerFtz=ftz, blocks=blocks)

def freeze(dll, work, output):
    source = Path(__file__).with_name("stereo_zero_native.c")
    image = work / "image.bin"
    image.write_bytes(mapped_image(dll.read_bytes()))
    native = work / "native"
    subprocess.run(["cc", *FLAGS, str(source), "-o", str(native)], check=True, timeout=30)
    cases = []
    for case in plans():
        cap = case["capacity"]
        wire = struct.pack("<4I", case["rate"], cap, len(case["blocks"]), case["callerFtz"])
        for block in case["blocks"]:
            wire += struct.pack("<IIIffIiI", block["frames"], block["bypass"], block["end"],
                                block["presence"], block["stereoizer"], block["shaper"], 0, block["reset"])
            wire += struct.pack("<%dI" % (cap * 2), *block["inputBits"])
        reply = subprocess.run(["unshare", "-Urn", str(native), "process", str(image)], input=wire,
                               capture_output=True, check=True, timeout=15).stdout
        record = 8 + (2 * cap + 22) * 4
        if len(reply) != record * len(case["blocks"]):
            raise ValueError("unexpected native record length")
        for index, block in enumerate(case["blocks"]):
            at = index * record
            if struct.unpack_from("<II", reply, at) != (block["frames"], block["end"]):
                raise ValueError("native changed no-ambience frames/EOF")
            data = struct.unpack_from("<%dI" % (cap * 2 + 22), reply, at + 8)
            block["outputBits"] = list(data[:block["frames"]]) + list(data[cap:cap + block["frames"]])
            block["stateBits"] = list(data[cap * 2:])
        cases.append(case)
    corpus = dict(schema="cumulus-se-no-ambience-v1", cases=cases)
    encoded = json.dumps(corpus, separators=(",", ":"), ensure_ascii=True).encode() + b"\n"
    output.write_bytes(gzip.compress(encoded, mtime=0))
    manifest = dict(schema=corpus["schema"], dllSha256=DLL_SHA, fixtureSha256=digest(output.read_bytes()),
                    oracleSourceSha256=digest(source.read_bytes()), generatorSha256=digest(Path(__file__).read_bytes()),
                    sourceReferences=["A82B0", "B34C0", "B28E0", "B3060", "A80D0", "A85B0", "A8550", "B3990"],
                    executedRoots=["B2030", "B1780", "B1AE0", "B34C0", "B3990", "A85B0", "A8550", "A80D0"],
                    initializationAdapter="host reproduces A82B0 ctor/init/reset order and bank1 using bounded allocator/context; A82B0 global allocator entry itself is not executed",
                    initialBank=1, activeBank=0, ambience=0, frames="1..capacity", channels=2,
                    capacities="4..4096, multiples of four", inputPacking="full-capacity planar f32 bits",
                    outputPacking="valid mid-transformed L then R; full padded tail not compared",
                    statePacking="parent side gain, applied presence, M then S five envelope/power pairs",
                    ftz="original wrapper enables FTZ during plugin processing; caller FTZ0/1 preserved",
                    adapters=["allocator/free", "block-context", "TEB stack lower bound", "PE mapping"],
                    untouched="original RX machine code, PE data/bank table, actual EQ/dynamics/environment initialization",
                    excludes=["DllMain/imports/Windows CPU-init", "sound device", "services/accounts", "ambience>0 PCM", "custom banks"],
                    compilerFlags=FLAGS, compiler=subprocess.check_output(["cc", "--version"], text=True).splitlines()[0],
                    platform=platform.platform(), caseCount=len(cases), blockCount=sum(len(c["blocks"]) for c in cases))
    output.with_suffix("").with_suffix(".manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"cases":len(cases), "blocks":manifest["blockCount"], "fixtureSha256":manifest["fixtureSha256"]}))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("the manual original oracle requires Linux x86_64")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="stereo-zero-oracle-", dir=args.output.parent) as temporary:
        freeze(args.dll, Path(temporary), args.output)

if __name__ == "__main__":
    main()
