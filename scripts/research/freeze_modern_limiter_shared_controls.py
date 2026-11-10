#!/usr/bin/env python3
"""Freeze old GEQ -> modern -> off/reapply on the same original SDK graph."""

import argparse
import copy
import hashlib
import json
import socket
from pathlib import Path

from freeze_modern_limiter_switch import CAPACITY, ROOT, SDK_SHA
from probe_modern_limiter_initial_transition import OriginalLocal, geq_flags


def freeze(dll, profile_name, profile, rate, mode):
    graph = OriginalLocal(dll, rate, None)
    quantum = CAPACITY
    graph.write(0, quantum)
    old_pcm = graph.read(31)
    graph.write(quantum, 511)
    graph.apply(profile)
    controls = {"initial": [geq_flags(graph), graph.flags()]}
    if mode == "initialCoalesced":
        graph.disable_master()
        controls["off"] = [geq_flags(graph), graph.flags()]
        graph.apply(profile)
        controls["reapply"] = [geq_flags(graph), graph.flags()]
    old_pcm += graph.read(quantum)
    graph.write(quantum + 511, quantum - 511)

    first_modern = graph.read(31)
    graph.write(2 * quantum, 257)
    if mode in ("settledOff", "coalesced"):
        graph.disable_master()
        controls["off"] = [geq_flags(graph), graph.flags()]
    if mode == "coalesced":
        graph.apply(profile)
        controls["reapply"] = [geq_flags(graph), graph.flags()]
    first_modern += graph.read(quantum)
    graph.write(2 * quantum + 257, quantum - 257)

    middle = graph.read(31)
    graph.write(3 * quantum, 1023)
    if mode == "settledOff":
        graph.apply(profile)
        controls["reapply"] = [geq_flags(graph), graph.flags()]
    middle += graph.read(quantum)
    graph.write(3 * quantum + 1023, quantum - 1023)

    final = graph.read(quantum)
    graph.write(4 * quantum, 7)
    graph.write(4 * quantum + 7, 0)
    final += graph.read(quantum, end=True)
    assert graph.read(quantum, end=True) == b""
    assert graph.inputs == [quantum] * 4 + [7]
    assert all(call == "0x64f40" for call in graph.native_calls)
    graph.check_text()
    pcm = (old_pcm, first_modern, middle, final)
    return {
        "profile": profile_name,
        "rate": rate,
        "mode": mode,
        "inputs": graph.inputs,
        "outputs": graph.outputs,
        "frames": [len(part) // 8 for part in pcm],
        "sha256": [hashlib.sha256(part).hexdigest() for part in pcm],
        "controls": controls,
        "nativeB0Setups": len(graph.native_calls),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    assert {name for _, name in socket.if_nameindex()} <= {"lo"}, "use a loopback-only namespace"
    assert args.dll.is_file() and args.dll.stat().st_size <= 8 * 1024 * 1024
    assert hashlib.sha256(args.dll.read_bytes()).hexdigest() == SDK_SHA
    corpus = ROOT / "core/tests/fixtures/effects-modern-limiter-graph-official.json"
    profiles = json.loads(corpus.read_text())["profiles"]
    profiles["defaultTiming"] = copy.deepcopy(profiles["nonzeroTone"])
    profiles["defaultTiming"]["aev2"]["limiter"].update(
        attack=0.005, release=0.1, threshold=0, outputgain=0,
    )
    cases = [
        freeze(args.dll, name, profile["aev2"], rate, mode)
        for name, profile in profiles.items()
        for rate in (44100, 48000)
        for mode in ("steady", "settledOff", "coalesced", "initialCoalesced")
    ]
    result = {"schema": "cumulus.modern-limiter-x64-shared-controls.v1", "sdkSha256": SDK_SHA, "cases": cases}
    with args.out.open("x") as target:
        target.write(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
