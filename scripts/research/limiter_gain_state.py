#!/usr/bin/env python3
"""Validate recovered default isolated/dominant-peak limiter arithmetic offline."""
import argparse
import hashlib
import json
import math
import platform
import struct
import subprocess
import tempfile
from pathlib import Path

from original_limiter_oracle import DLL_SHA, mapped_image

CAPACITY = 256
SCALARS = (
    "threshold", "release", "gain", "inputGain", "outputGain", "overPeakSum",
    "inverseRatio", "adaptiveRatioFactor", "gainStep",
)
INDICES = (
    "infiniteRatio", "overPeakCount", "markedRingSlot", "ringScalars", "ringWrite",
    "startupFrames", "tailFrames", "lookahead", "eventHead", "eventCount", "markerCount",
)
TRACE = struct.Struct("<9d11i")


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def packet_audio(channels, length, peaks, cuts):
    cuts = sorted({0, length, *cuts, *range(0, length, CAPACITY)})
    assert cuts[0] == 0 and cuts[-1] == length
    counts = [b - a for a, b in zip(cuts, cuts[1:])]
    commands = [(count, int(index == len(counts) - 1)) for index, count in enumerate(counts)]
    commands += [(0, 1)] * 4
    assert len(commands) < 256
    samples = [[0.25] * length for _ in range(channels)]
    for index, amplitude in peaks.items():
        samples[-1][index] = amplitude
    return commands, samples


def run(child, image, rate, samples, commands):
    channels = len(samples)
    packet = bytearray(struct.pack("<4I", rate, channels, CAPACITY, len(commands)))
    offset = 0
    for valid, end in commands:
        packet.extend(struct.pack("<3I", valid, end, 0))
        for channel in samples:
            block = channel[offset:offset + valid] + [0.0] * (CAPACITY - valid)
            packet.extend(struct.pack(f"<{CAPACITY}f", *block))
        offset += valid
    result = subprocess.run([child, image, "--state"], input=packet, capture_output=True,
                            check=True, timeout=5).stdout
    row = 8 + 4 * channels * CAPACITY + TRACE.size
    assert len(result) == row * len(commands)
    values = [[] for _ in range(channels)]
    trace = []
    for index, (valid_input, end_input) in enumerate(commands):
        start = index * row
        valid, end = struct.unpack_from("<2I", result, start)
        state = dict(zip(SCALARS + INDICES, TRACE.unpack_from(result, start + row - TRACE.size)))
        assert valid <= CAPACITY and end in (0, 1)
        trace.append({"inputFrames": valid_input, "inputEnd": end_input, "outputFrames": valid,
                      "outputEnd": end, **state})
        for ch in range(channels):
            values[ch].extend(struct.unpack_from(f"<{valid}f", result,
                                                  start + 8 + 4 * ch * CAPACITY))
    return values, trace


def model(rate, samples, dominant_peaks, queued):
    """Exact for these traces only; does not implement the multi-event scheduler."""
    length = len(samples[0])
    ring_frames = int(rate * f32(0.005))
    release_frames = rate * f32(0.1)
    queued_targets = set(queued.values())
    attacks = {peak - (ring_frames - 1): (peak, target)
               for peak, target in dominant_peaks.items() if peak not in queued_targets}
    gain, step, scheduled = 1.0, 0.0, None
    output = [[] for _ in samples]
    for i in range(length):
        if i in attacks:
            scheduled, target = attacks[i]
            step = (target - gain) / ring_frames
        gain += step
        if i == scheduled:
            gain = dominant_peaks[i]
            step = (1.0 - gain) / release_frames
            scheduled = queued.get(i)
            if scheduled is not None:
                step = min(step, (dominant_peaks[scheduled] - gain) / (scheduled - i))
        for channel, source in zip(output, samples):
            channel.append(f32(max(-1.0, min(1.0, source[i] * gain))))
        # The output for the crossing frame has already been scaled with the
        # overshoot. The stored gain/slope are reset for the following frame.
        if gain >= 1.0 or 1.0 - gain < 1e-13:
            gain, step = 1.0, 0.0
    return output


def verify_case(child, image, rate, channels, label, peaks, targets, length, queued=None):
    cuts = [i for peak in peaks for i in (peak, peak + 1)]
    commands, samples = packet_audio(channels, length, peaks, cuts)
    actual, trace = run(child, image, rate, samples, commands)
    expected = model(rate, samples, targets, queued or {})
    pcm = b"".join(struct.pack(f"<{length}f", *channel) for channel in actual)
    reference_pcm = b"".join(struct.pack(f"<{length}f", *channel) for channel in expected)
    assert pcm == reference_pcm
    ring = int(rate * f32(0.005))
    input_clock = 0
    milestones = []
    for state in trace:
        assert state["ringScalars"] == ring * channels
        if not state["inputEnd"]:
            input_clock += state["inputFrames"]
            assert state["ringWrite"] == (input_clock * channels) % (ring * channels)
            assert state["startupFrames"] == max(0, ring - 1 - input_clock)
            assert state["tailFrames"] == ring - 1
            if input_clock - 1 in peaks:
                milestones.append({"afterInputIndex": input_clock - 1, **state})
    assert len(milestones) == len(peaks)
    over = [i for i, x in enumerate(actual[0]) if x > 0.25]
    if label == "isolated":
        peak = next(iter(peaks))
        assert over == [peak + math.ceil(rate * f32(0.1))]
        assert milestones[0]["eventCount"] == 1 and milestones[0]["overPeakCount"] == 1
    elif label == "stronger_then_weaker":
        assert [row["eventCount"] for row in milestones] == [1, 1, 1]
        assert [row["overPeakCount"] for row in milestones] == [1, 2, 3]
    else:
        assert label == "two_events"
        assert [row["eventCount"] for row in milestones] == [1, 2]
        assert [row["overPeakCount"] for row in milestones] == [1, 2]
    return {
        "label": label, "rate": rate, "channels": channels, "samples": length * channels,
        "ringFrames": ring, "startupDelayFrames": ring - 1, "peaks": list(peaks.items()),
        "dominantPeaks": list(targets.items()), "queuedAtPeak": list((queued or {}).items()),
        "milestones": milestones,
        "aboveUnityPilot": [[i, actual[0][i]] for i in over],
        "validPlanarPcmSha256": hashlib.sha256(pcm).hexdigest(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise SystemExit("requires Linux x86-64")
    if args.report.exists():
        raise FileExistsError(args.report)
    with tempfile.TemporaryDirectory(prefix="limiter-gain-state-") as directory:
        directory = Path(directory)
        child, image = directory / "oracle", directory / "official.image"
        image.write_bytes(mapped_image(args.dll))
        source = Path(__file__).with_name("native_limiter_oracle.c")
        subprocess.run(["cc", "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror",
                        "-ffp-contract=off", source, "-o", child], check=True, timeout=30)
        cases = []
        for rate in (44100, 48000, 96000):
            for channels in (1, 2):
                peak = 829
                cases.append(verify_case(child, image, rate, channels, "isolated",
                                         {peak: -2.0}, {peak: 0.5}, peak + round(rate * 0.12)))
        cases.append(verify_case(child, image, 48000, 2, "stronger_then_weaker",
                                 {768: 1.5, 833: 3.0, 898: 1.25},
                                 {768: 1.0 / 1.5, 833: 1.0 / 3.0}, 1600))
        cases.append(verify_case(child, image, 48000, 2, "two_events",
                                 {857: f32(1.8), 1000: 3.0},
                                 {857: 1.0 / f32(1.8), 1000: 1.0 / 3.0}, 1700, {857: 1000}))
    report = {
        "dllSha256": DLL_SHA, "exactValidPcm": True, "fullSchedulerRecovered": False,
        "networkRequests": 0, "cases": cases,
    }
    with args.report.open("x", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps({"cases": len(cases), "samples": sum(case["samples"] for case in cases)}))


if __name__ == "__main__":
    main()
