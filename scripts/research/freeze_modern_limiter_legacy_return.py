#!/usr/bin/env python3
"""Freeze official explicit close and legacy return on the same effects graph."""

import argparse
import copy
import hashlib
import json
import socket
import struct
from pathlib import Path

from freeze_modern_limiter_switch import CAPACITY, ROOT, SDK_SHA
from probe_modern_limiter_initial_transition import OriginalLocal, geq_flags


class OriginalReturn(OriginalLocal):
    def legacy_return(self):
        # M3091 local old JSON: a different GEQ curve, BT/SE/RVB off.
        self.call(0x158)
        self.vm.u.mem_write(self.params, struct.pack("<10f", 0, 0, -4.5, *([0] * 7)))
        self.call(0xA0, self.params)
        self.call(0x98, 1)
        self.call(0xA8, 0)
        self.call(0x88, 0)
        self.call(0x48, 0)
        self.call(0x160)


def freeze(dll, name, profile, rate, mode):
    graph = OriginalReturn(dll, rate, None)
    q = CAPACITY
    graph.write(0, q)
    old = graph.read(31)
    graph.write(q, 511)
    graph.apply(profile)
    old += graph.read(q)
    graph.write(q + 511, q - 511)

    modern = graph.read(31)
    graph.write(2 * q, 257)
    controls = {}
    if mode != "directLegacy":
        graph.disable_master()
        controls["off"] = [geq_flags(graph), graph.flags()]
    if mode in ("directLegacy", "closeThenLegacyCoalesced"):
        graph.legacy_return()
        controls["legacy"] = [geq_flags(graph), graph.flags()]
    modern += graph.read(q)
    graph.write(2 * q + 257, q - 257)

    middle = graph.read(31)
    graph.write(3 * q, 1023)
    if mode == "closeThenLegacySettled":
        graph.legacy_return()
        controls["legacy"] = [geq_flags(graph), graph.flags()]
    middle += graph.read(q)
    graph.write(3 * q + 1023, q - 1023)

    final = graph.read(q)
    graph.write(4 * q, 7)
    graph.write(4 * q + 7, 0)
    final += graph.read(q, end=True)
    assert graph.read(q, end=True) == b""
    assert graph.inputs == [q] * 4 + [7]
    assert all(call == "0x64f40" for call in graph.native_calls)
    graph.check_text()
    pcm = (old, modern, middle, final)
    return {
        "profile": name,
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
        for mode in ("closeDry", "directLegacy", "closeThenLegacyCoalesced", "closeThenLegacySettled")
    ]
    result = {"schema": "cumulus.modern-limiter-x64-legacy-return.v1", "sdkSha256": SDK_SHA, "cases": cases}
    with args.out.open("x") as target:
        target.write(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
