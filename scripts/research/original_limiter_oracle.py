#!/usr/bin/env python3
"""Offline synthetic PCM against the pinned Windows x64 LookaheadLimiter."""
import argparse
import hashlib
import json
import math
import platform
import struct
import subprocess
import tempfile
from pathlib import Path

DLL_SHA = "c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be"
IMAGE_SIZE = 0x3A3000
IMAGE_FILE_SIZE = 0x3A2400


def sha(data):
    return hashlib.sha256(data).hexdigest()


def mapped_image(path):
    with path.open("rb") as stream:
        data = stream.read(8 * 1024 * 1024)
    if sha(data) != DLL_SHA:
        raise ValueError("original neaudioeffects.dll hash mismatch")
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    assert data[pe:pe + 4] == b"PE\0\0"
    machine, sections = struct.unpack_from("<HH", data, pe + 4)
    opt_len = struct.unpack_from("<H", data, pe + 20)[0]
    opt = pe + 24
    assert machine == 0x8664 and struct.unpack_from("<H", data, opt)[0] == 0x20B
    assert struct.unpack_from("<Q", data, opt + 24)[0] == 0x180000000
    size, headers = struct.unpack_from("<II", data, opt + 56)
    assert size == IMAGE_SIZE and headers <= len(data)
    image = bytearray(size)
    image[:headers] = data[:headers]
    for i in range(sections):
        _, rva, count, offset = struct.unpack_from("<4I", data, opt + opt_len + 40 * i + 8)
        assert offset + count <= len(data) and rva + count <= size
        image[rva:rva + count] = data[offset:offset + count]
    return image[:IMAGE_FILE_SIZE]


def execute(child, image, rate, channels, capacity, commands, sample):
    packet = bytearray(struct.pack("<4I", rate, channels, capacity, len(commands)))
    offset = 0
    dry = [[] for _ in range(channels)]
    for valid, end, action in commands:
        packet.extend(struct.pack("<3I", valid, end, action))
        for ch in range(channels):
            values = [sample(offset + i, ch) if i < valid else 0.0 for i in range(capacity)]
            pcm = struct.pack(f"<{capacity}f", *values)
            packet.extend(pcm)
            dry[ch].extend(struct.unpack(f"<{capacity}f", pcm)[:valid])
        offset += valid
    run = subprocess.run([child, image], input=packet, capture_output=True, timeout=5, check=False)
    if run.returncode:
        raise RuntimeError(f"bounded limiter child failed ({run.returncode})")
    row_size = 8 + capacity * channels * 4
    assert len(run.stdout) == row_size * len(commands)
    frames, output = [], [[] for _ in range(channels)]
    for index in range(len(commands)):
        start = index * row_size
        valid, end = struct.unpack_from("<2I", run.stdout, start)
        assert valid <= capacity and end in (0, 1)
        frames.append([valid, end])
        for ch in range(channels):
            values = struct.unpack_from(f"<{valid}f", run.stdout, start + 8 + ch * capacity * 4)
            assert all(math.isfinite(v) for v in values)
            output[ch].extend(values)
    return frames, output, dry, sha(run.stdout)


def continuous_cases(child, image):
    cases = []
    for rate in (44100, 48000, 96000):
        for channels in (1, 2):
            for capacity in (256, 1024):
                counts = [capacity] * 12 + [73]
                commands = [(n, int(i == 12), 0) for i, n in enumerate(counts)] + [(0, 1, 0)] * 8
                for pattern in ("quiet", "overload", "impulse"):
                    def sample(i, ch):
                        if pattern == "impulse":
                            return (1.5 if ch == 0 else -1.8) if i in (61, capacity + 61) else 0
                        scale = 0.1 if pattern == "quiet" else 1.6
                        return scale * math.sin(2 * math.pi * (440 + 217 * ch) * i / rate)
                    frames, audio, dry, output_hash = execute(child, image, rate, channels, capacity, commands, sample)
                    delay = {44100: 219, 48000: 238, 96000: 478}[rate]
                    assert frames[0][0] == max(0, capacity - delay)
                    if delay > capacity:
                        assert frames[1][0] == 2 * capacity - delay
                    finish = next(i for i, (_, end) in enumerate(frames) if end)
                    assert finish >= len(counts) - 1
                    assert sum(n for n, _ in frames[:finish + 1]) == sum(counts)
                    assert all(n == 0 and end == 1 for n, end in frames[finish + 1:])
                    assert all(len(wet) == len(original) for wet, original in zip(audio, dry))
                    peak = max(abs(v) for ch in audio for v in ch)
                    if pattern == "quiet":
                        assert all(max(abs(a - b) for a, b in zip(wet, original)) <= 1e-7
                                   for wet, original in zip(audio, dry))
                    if pattern in ("overload", "impulse"):
                        assert peak == 1.0
                    valid_pcm = b"".join(struct.pack(f"<{len(ch)}f", *ch) for ch in audio)
                    cases.append({
                        "rate": rate, "channels": channels, "capacity": capacity, "pattern": pattern,
                        "in": counts, "out": frames[:finish + 1], "inputSamples": len(dry[0]) * channels,
                        "startupDelayFrames": delay, "peak": peak, "packetSha256": output_hash,
                        "validPlanarPcmSha256": sha(valid_pcm),
                    })
    return cases


def transitions(child, image):
    rate, channels, capacity = 48000, 2, 256
    commands = [(capacity, 0, action) for action in (0, 0, 0, 1, 0, 2, 0, 3, 0)]
    frames, _, _, output_hash = execute(child, image, rate, channels, capacity, commands,
                                        lambda i, ch: 0.2 * math.sin(i * (0.17 + 0.02 * ch)))
    assert frames == [[n, 0] for n in (18, 256, 256, 256, 256, 18, 256, 18, 256)]
    return {"rate": rate, "channels": channels, "capacity": capacity,
            "actions": [action for _, _, action in commands], "out": frames, "sha256": output_hash}


def main():
    args = argparse.ArgumentParser(description=__doc__)
    args.add_argument("--dll", type=Path, required=True)
    args.add_argument("--report", type=Path, required=True)
    args = args.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("requires Linux x86-64")
    if args.report.exists():
        raise FileExistsError(args.report)
    source = Path(__file__).with_name("native_limiter_oracle.c")
    with tempfile.TemporaryDirectory(prefix="original-limiter-") as directory:
        directory = Path(directory)
        image = directory / "official.image"
        child = directory / "oracle"
        image.write_bytes(mapped_image(args.dll))
        subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-ffp-contract=off", source, "-o", child], check=True, timeout=30)
        cases = continuous_cases(child, image)
        switches = transitions(child, image)
    report = {
        "dllSha256": DLL_SHA, "childSourceSha256": sha(source.read_bytes()),
        "driverSha256": sha(Path(__file__).read_bytes()), "originalDsp": True,
        "windowsImports": False, "fullGraphExecuted": False, "networkRequests": 0,
        "seams": {"0x2842": "bounded calloc", "0x2d15": "checked pointer-to-pointer free",
                  "0x9421": "bounded memmove", "0x6c4e": "bounded memset"},
        "cases": cases, "transitions": switches,
    }
    with args.report.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"cases": len(cases), "transitionCases": 1,
                      "inputSamples": sum(x["inputSamples"] for x in cases)}))


if __name__ == "__main__":
    main()
