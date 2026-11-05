#!/usr/bin/env python3
"""Manually freeze original fixed-bank1 environment PCM, flow and history.

Requires the local hash-pinned DLL and Linux x86_64 namespaces. This script
and the original binary are never required by public CI or product runtime.
No DllMain, services, user media, device output or network is invoked.
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

from freeze_stereo_zero import DLL_SHA, FLAGS, mapped_image


def digest(data):
    return hashlib.sha256(data).hexdigest()


def bits(values):
    return list(struct.unpack("<%dI" % len(values), struct.pack("<%df" % len(values), *values)))


def plans():
    shapes = [(8000, 64), (44100, 256), (48000, 512), (96000, 1024), (384000, 4096)]
    shapes += [(48000, 4), (44100, 8)]
    for rate, cap in shapes:
        for ftz in (0, 1):
            rng = random.Random(rate + cap)
            # Small-capacity cases cover short/tail arithmetic without a huge
            # public fixture. The private differential matrix also drains them.
            end_index = 18 + (math.ceil(rate / (4 * cap)) if cap >= 64 else 0)
            count = end_index + (math.ceil(rate / cap) + 4 if cap >= 64 else 4)
            blocks = []
            for index in range(count):
                frames = [cap, cap - 1, 1, 3, 0, min(7, cap)][index % 6] if index < 18 else cap if index < end_index else 0
                end = index >= end_index
                pcm = [math.sin((index * cap + i) * .131) * .12 + rng.uniform(-.01, .01)
                       for i in range(cap)]
                pcm += [math.cos((index * cap + i) * .037) * .09 + rng.uniform(-.01, .01)
                        for i in range(cap)]
                if index == 0:
                    pcm = [0.] * (2 * cap)
                    pcm[0] = .5
                if index == 2:
                    pcm = [(-1 if i % 2 else 1) * (1e-40 if i % 3 else 2e-38)
                           for i in range(2 * cap)]
                if 18 <= index < end_index:
                    pcm = [((i + index) % 31 - 15) * .0078125 for i in range(2 * cap)]
                if end:
                    pcm = [0.] * (2 * cap)
                # Padding is deliberately nonzero: SIMD prefix writes are
                # bounded by capacity, and only returned valid PCM is frozen.
                resets = (7, 13, end_index + math.ceil(rate * .6 / cap)) if cap >= 64 else (7, 13, 21)
                blocks.append(dict(frames=frames, end=end, reset=index in resets, inputBits=bits(pcm)))
            yield dict(name=f"{rate}-{cap}-{ftz}", rate=rate, capacity=cap, callerFtz=ftz, endIndex=end_index, blocks=blocks)


def freeze(dll, work, output):
    source = Path(__file__).with_name("ambience_one_native.c")
    image = work / "image.bin"
    image.write_bytes(mapped_image(dll.read_bytes()))
    native = work / "native"
    subprocess.run(["cc", *FLAGS, str(source), "-o", str(native)], check=True, timeout=30)
    cases = []
    for case in plans():
        cap = case["capacity"]
        wire = struct.pack("<4I", case["rate"], cap, len(case["blocks"]), case["callerFtz"])
        for block in case["blocks"]:
            wire += struct.pack("<III", block["frames"], block["end"], block["reset"])
            wire += struct.pack("<%dI" % (cap * 2), *block["inputBits"])
        reply = subprocess.run(["unshare", "-Urn", str(native), "process", str(image)],
                               input=wire, capture_output=True, check=True, timeout=30).stdout
        record = 8 + (2 * cap + 87 + 70) * 4
        if len(reply) != record * len(case["blocks"]):
            raise ValueError("unexpected original record length")
        for index, block in enumerate(case["blocks"]):
            at = index * record
            frames, end = struct.unpack_from("<II", reply, at)
            if frames > cap or end > 1:
                raise ValueError("invalid original flow")
            pcm = struct.unpack_from("<%dI" % (cap * 2), reply, at + 8)
            block["outputFrames"], block["outputEnd"] = frames, bool(end)
            valid_pcm = list(pcm[:frames]) + list(pcm[cap:cap + frames])
            block["outputSha256"] = digest(struct.pack("<%dI" % len(valid_pcm), *valid_pcm))
            # Keep explicit PCM around input/reset/EOF transitions and a late
            # audible tail. Other full blocks retain an exact SHA256 receipt,
            # rather than adding megabytes of repetitive tail vectors.
            if index < 20 or abs(index - case["endIndex"]) < 3 or index == len(case["blocks"]) // 2 or index >= len(case["blocks"]) - 3:
                block["outputBits"] = valid_pcm
            block["indices"] = list(struct.unpack_from("<87I", reply, at + 8 + 8 * cap))
            block["historyBits"] = list(struct.unpack_from("<70I", reply, at + 8 + 8 * cap + 348))
        if cap >= 64 and not case["blocks"][-1]["outputEnd"]:
            raise ValueError("original EOF did not finish")
        cases.append(case)
    corpus = dict(schema="cumulus-se-bank1-environment-v1", cases=cases)
    output.write_bytes(gzip.compress(json.dumps(corpus, separators=(",", ":")).encode() + b"\n", mtime=0))
    manifest = dict(schema=corpus["schema"], dllSha256=DLL_SHA,
                    fixtureSha256=digest(output.read_bytes()), oracleSourceSha256=digest(source.read_bytes()),
                    generatorSha256=digest(Path(__file__).read_bytes()),
                    initialization="original B2030/B1780/B1AE0/B34C0/B3990; original bank1 table retained only inside local oracle",
                    processing=["A85B0 selects bank1 once", "B6EB0", "BA4C0", "BE020 reset"],
                    arithmeticHooks=[], adapters=["bounded calloc/free", "capacity context", "TEB stack bound", "PE mapping"],
                    inputPacking="full-capacity stereo planar f32 bits; variable valid frames, end and reset",
                    outputPacking="returned valid L then R f32 bits; padding excluded",
                    statePacking="87 numeric indices: tail3, AP4, 16*(ring4+delay1); 70 floats: damping2*3, feedback16*4",
                    ftz="environment inherits and preserves caller FTZ0/1; outer SE host policy is not applied",
                    exclusions=["DllMain/Windows CPU initialization", "complete SE graph", "arbitrary banks/parameters", "ER", "extra EQ", "live playback", "physical device", "accounts/services", "Windows libm equivalence"],
                    compilerFlags=FLAGS, compiler=subprocess.check_output(["cc", "--version"], text=True).splitlines()[0],
                    platform=platform.platform(), caseCount=len(cases), blockCount=sum(len(c["blocks"]) for c in cases))
    output.with_suffix("").with_suffix(".manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"cases": len(cases), "blocks": manifest["blockCount"], "fixtureSha256": manifest["fixtureSha256"]}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("manual original oracle requires Linux x86_64")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="ambience-one-oracle-", dir=args.output.parent) as temporary:
        freeze(args.dll, Path(temporary), args.output)


if __name__ == "__main__":
    main()
