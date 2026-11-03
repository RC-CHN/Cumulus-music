#!/usr/bin/env python3
"""Compare a bounded default limiter scheduler to the original node offline."""
import argparse
import hashlib
import json
import math
import platform
import random
import struct
import subprocess
import tempfile
from pathlib import Path

from limiter_gain_state import CAPACITY, f32, packet_audio, run
from original_limiter_oracle import DLL_SHA, mapped_image


class SentinelCollision(Exception):
    pass


def default_scheduler(rate, samples):
    """Normal default continuous processing; reject rather than read ring[-1]."""
    channels = len(samples)
    ring_frames = int(rate * f32(0.005))
    slots = ring_frames * channels
    delay = ring_frames - 1
    release = rate * f32(0.1)
    ring = [[0.0] * channels for _ in range(ring_frames)]
    write = head = peak_count = 0
    events = []
    gain, slope, peak_sum = 1.0, 0.0, 0.0
    output = [[] for _ in samples]
    states = []
    length = len(samples[0])
    for time in range(length + delay):
        frame = [channel[time] if time < length else 0.0 for channel in samples]
        ring[write] = frame
        peak = max(map(abs, frame))
        if peak > 1.0:
            peak_sum += peak
            peak_count += 1
            target = 1.0 / peak
            attack = (target - gain) / ring_frames
            event_release = (1.0 - target) / release
            if attack < slope:
                slope, events, head = attack, [(write, event_release)], 0
            else:
                for index, (slot, old_slope) in enumerate(events):
                    distance = (write - slot) % ring_frames
                    assert distance > 0
                    old_target = 1.0 / max(map(abs, ring[slot]))
                    secant = (target - old_target) / distance
                    if secant < old_slope:
                        events[index:] = [(slot, secant), (write, event_release)]
                        break
        if len(events) >= slots:
            raise SentinelCollision(time)

        read = (write + 1) % ring_frames
        delayed = ring[read]
        delayed_peak = max(map(abs, delayed))
        if delayed_peak > 1.0:
            peak_sum -= delayed_peak
            peak_count -= 1
        gain += slope
        if time >= delay:
            for channel, sample in zip(output, delayed):
                channel.append(max(-1.0, min(1.0, sample * gain)))

        if events and events[0][0] == read:
            slope = (1.0 - gain) / release
            if len(events) > 1:
                next_slot = events[1][0]
                next_target = 1.0 / max(map(abs, ring[next_slot]))
                slope = min(slope, (next_target - gain) / ((next_slot - read) % ring_frames))
            events.pop(0)
            head = (head + 1) % slots
        if gain > 1.0:
            gain, slope, events, head = 1.0, 0.0, [], 0
        if gain <= 0.0:
            gain = float.fromhex("0x1.c25c268497682p-44")
            slope = 0.9999999999999 / release
        if 1.0 - gain < 1e-13:
            gain = 1.0
        if abs(slope) < 1e-14:
            slope = 0.0
        write = read
        states.append((gain, slope, write * channels, head, len(events), peak_sum, peak_count,
                       [(slot * channels, step) for slot, step in events[:16]]))
    return output, states


def compare(child, image, name, rate, samples, commands):
    actual, trace = run(child, image, rate, samples, commands, queue=True)
    reference, states = default_scheduler(rate, samples)
    def pack(channels):
        return b"".join(struct.pack(f"<{len(ch)}f", *ch) for ch in channels)

    data = pack(actual)
    assert data == pack(reference), (name, "PCM")
    processed, previous_head, head_wraps = 0, 0, 0
    length = len(samples[0])
    delay = trace[0]["ringScalars"] // len(samples) - 1
    for state in trace:
        tail = delay - max(0, processed - length)
        new = state["inputFrames"]
        if state["inputEnd"]:
            new += min(tail, CAPACITY - new)
        processed += new
        assert state["sentinel"] == -1 and state["markedRingSlot"] == -1 and state["markerCount"] == 0
        actual_state = tuple(state[field] for field in (
            "gain", "gainStep", "ringWrite", "eventHead", "eventCount", "overPeakSum", "overPeakCount", "queuedEvents"
        ))
        assert actual_state == states[processed - 1], (name, processed, actual_state, states[processed - 1])
        if state["eventHead"] < previous_head and state["eventHead"] != 0:
            head_wraps += 1
        previous_head = state["eventHead"]
    assert processed == len(states)
    return {"case": name, "rate": rate, "channels": len(samples),
            "samples": sum(map(len, samples)), "blocks": len(trace),
            "maxEvents": max(state["eventCount"] for state in trace),
            "headWrapObserved": bool(head_wraps),
            "validPlanarPcmSha256": hashlib.sha256(data).hexdigest()}


def collision(child, image):
    # L=220 mono slots, one event per sample, before the first event can pop.
    rate, ring = 44100, 220
    commands = [(1, 0)] * ring
    _, trace = run(child, image, rate, [[1.5] * ring], commands, queue=True)
    last = trace[-1]
    assert last["eventHead"] == 0 and last["eventCount"] == last["ringScalars"] == ring
    assert last["sentinel"] == last["queuedEvents"][0][0] == -1
    try:
        default_scheduler(rate, [[1.5] * ring])
    except SentinelCollision as error:
        assert error.args == (ring - 1,)
    else:
        raise AssertionError("safe model allowed the sentinel to overwrite the queue head")

    command = struct.pack("<3I256f", 1, 0, 0, 1.5, *([0.0] * 255))
    packet = struct.pack("<4I", rate, 1, CAPACITY, ring + 1) + command * (ring + 1)
    guard = subprocess.run([child, image, "--guard-ring"], input=packet, capture_output=True,
                           timeout=5, check=False)
    assert guard.returncode == 81
    assert guard.stderr == b"original limiter read ring[-1] at RVA 0x64327\n"
    return {"rate": rate, "channels": 1, "amplitude": 1.5, "collisionAtInputIndex": ring - 1,
            "headIndex": 0, "headSlot": -1, "queueCount": ring, "readAtNextInputRva": "0x64327",
            "guardExit": guard.returncode, "safeModelRejected": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("requires Linux x86-64")
    if args.report.exists():
        raise FileExistsError(args.report)
    random_input = random.Random(930)
    cases = []
    with tempfile.TemporaryDirectory(prefix="limiter-scheduler-") as directory:
        directory = Path(directory)
        image, child = directory / "official.image", directory / "oracle"
        image.write_bytes(mapped_image(args.dll))
        source = Path(__file__).with_name("native_limiter_oracle.c")
        subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-ffp-contract=off", source, "-o", child], check=True, timeout=30)
        for rate in (44100, 48000, 96000):
            for channels in (1, 2):
                for case in range(10):
                    peaks = {i: random_input.choice([1.015625, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0])
                             for i in random_input.sample(range(2200), 5 + case * 6)}
                    commands, samples = packet_audio(channels, 2200, peaks,
                                                     [i for k in peaks for i in (k, k + 1)])
                    cases.append(compare(child, image, f"pulse-chain-{case}", rate, samples, commands))
                for name in ("dense-sine", "dense-noise", "stereo-plateau"):
                    if name == "stereo-plateau" and channels == 1:
                        continue
                    commands, _ = packet_audio(channels, 3000, {}, range(21, 3000, 43))
                    if name == "dense-noise":
                        samples = [[f32(random_input.uniform(-3.0, 3.0)) for _ in range(3000)]
                                   for _ in range(channels)]
                    elif name == "dense-sine":
                        samples = [[f32(2.0 * math.sin(0.1 * i + 0.7 * c)) for i in range(3000)]
                                   for c in range(channels)]
                    else:
                        samples = [[1.5] * 3000 for _ in range(channels)]
                    cases.append(compare(child, image, name, rate, samples, commands))
        boundary = collision(child, image)
    assert any(case["maxEvents"] >= 220 for case in cases)
    assert all(case["headWrapObserved"] for case in cases if case["case"] == "stereo-plateau")
    report = {"dllSha256": DLL_SHA, "exactValidPcmAndTracedState": True, "networkRequests": 0,
              "noRenderer": True, "matchedCases": cases, "sentinelBoundary": boundary}
    with args.report.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"cases": len(cases), "samples": sum(case["samples"] for case in cases),
                      "sentinelCollision": boundary}))


if __name__ == "__main__":
    main()
