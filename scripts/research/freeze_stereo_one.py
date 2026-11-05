#!/usr/bin/env python3
"""Manually freeze the real fixed-bank1 SE parent, not a material/player.

Uses the pinned local DLL in a closed namespace. Inputs are the existing
synthetic environment corpus, avoiding a second copy of its input PCM.
"""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import platform
import struct
import subprocess
import tempfile

from freeze_stereo_zero import DLL_SHA, FLAGS, mapped_image


def digest(data):
    return hashlib.sha256(data).hexdigest()


def freeze(dll, inputs, work, output):
    source = Path(__file__).with_name("stereo_one_native.c")
    original = json.loads(gzip.decompress(inputs.read_bytes()))
    if original["schema"] != "cumulus-se-bank1-environment-v1":
        raise ValueError("unsupported synthetic inputs")
    image = work / "image.bin"
    image.write_bytes(mapped_image(dll.read_bytes()))
    native = work / "native"
    subprocess.run(["cc", *FLAGS, str(source), "-o", str(native)], check=True, timeout=30)
    cases = []
    for seed in original["cases"]:
        cap = seed["capacity"]
        case = {k: seed[k] for k in ("name", "rate", "capacity", "callerFtz")}
        wire = struct.pack("<4I", case["rate"], cap, len(seed["blocks"]), case["callerFtz"])
        blocks = []
        for index, block in enumerate(seed["blocks"]):
            presence = [0., 5., -10., 10., -3., 0.][index % 6]
            stereoizer = [0., 2.5, 10., 5., 0., 8.][index % 6]
            shaper = index % 7 != 2
            bypass = index in (5, 6, 16, 17, seed["endIndex"] + 3, seed["endIndex"] + 4)
            wire += struct.pack("<IIIffIiI", block["frames"], bypass, block["end"],
                                presence, stereoizer, shaper, 1, block["reset"])
            wire += struct.pack("<%dI" % (cap * 2), *block["inputBits"])
            blocks.append(dict(presence=presence, stereoizer=stereoizer, shaper=shaper, bypass=bypass))
        reply = subprocess.run(["unshare", "-Urn", str(native), "process", str(image)],
                               input=wire, capture_output=True, check=True, timeout=30).stdout
        record = 8 + (cap * 2 + 179) * 4
        if len(reply) != record * len(blocks):
            raise ValueError("unexpected original record length")
        for index, block in enumerate(blocks):
            at = index * record
            frames, end = struct.unpack_from("<II", reply, at)
            if frames > cap or end > 1:
                raise ValueError("invalid original flow")
            data = struct.unpack_from("<%dI" % (cap * 2 + 179), reply, at + 8)
            pcm = list(data[:frames]) + list(data[cap:cap + frames])
            block.update(outputFrames=frames, outputEnd=bool(end),
                         outputSha256=digest(struct.pack("<%dI" % len(pcm), *pcm)),
                         parentBits=list(data[cap * 2:cap * 2 + 22]),
                         indices=list(data[cap * 2 + 22:cap * 2 + 109]),
                         historyBits=list(data[cap * 2 + 109:]))
            if "outputBits" in seed["blocks"][index]:
                block["outputBits"] = pcm
        if cap >= 64 and not blocks[-1]["outputEnd"]:
            raise ValueError("original SE EOF did not finish")
        case["blocks"] = blocks
        cases.append(case)
    corpus = dict(schema="cumulus-se-bank1-parent-v1", inputFixtureSha256=digest(inputs.read_bytes()), cases=cases)
    output.write_bytes(gzip.compress(json.dumps(corpus, separators=(",", ":")).encode() + b"\n", mtime=0))
    manifest = dict(schema=corpus["schema"], dllSha256=DLL_SHA, inputFixtureSha256=corpus["inputFixtureSha256"],
                    fixtureSha256=digest(output.read_bytes()), oracleSourceSha256=digest(source.read_bytes()),
                    generatorSha256=digest(Path(__file__).read_bytes()),
                    initialization="original B2030/B1780/B1AE0/B34C0/B3990; fixed initial bank1",
                    executedRoots=["A85B0 ambience1", "A8550 bypass", "A80D0", "B28E0", "B3990 reset", "B6EB0 environment"],
                    arithmeticHooks=[], adapters=["bounded calloc/free", "capacity context", "TEB stack bound", "PE mapping"],
                    outputPacking="valid L then R PCM bits/SHA; parent22 + environment indices87/history70",
                    zeroFrames="real C0650 do-while once per channel using retained initialized band-cache first samples",
                    exclusions=["full material graph", "banks other than1", "ER", "extra EQ", "live player/UI", "Windows libm equivalence", "accounts/services/device output"],
                    compilerFlags=FLAGS, compiler=subprocess.check_output(["cc", "--version"], text=True).splitlines()[0],
                    platform=platform.platform(), caseCount=len(cases), blockCount=sum(len(c["blocks"]) for c in cases))
    output.with_suffix("").with_suffix(".manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps({"cases": len(cases), "blocks": manifest["blockCount"], "fixtureSha256": manifest["fixtureSha256"]}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--inputs", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        parser.error("manual original oracle requires Linux x86_64")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="stereo-one-oracle-", dir=args.output.parent) as temporary:
        freeze(args.dll, args.inputs, Path(temporary), args.output)


if __name__ == "__main__":
    main()
