#!/usr/bin/env python3
"""Compare original in-place local-to-modern apply to a fresh SDK graph."""
import argparse
import hashlib
import json
import socket
import struct
from pathlib import Path

from freeze_modern_limiter_switch import CAPACITY, ROOT, SDK_SHA, Original


class OriginalLocal(Original):
    def apply(self, profile):
        if profile is not None:
            return super().apply(profile)
        # The recovered old-schema GEQ-only local material, with other
        # local M/3091 components off. No audio has run after Init.
        self.call(0x158)
        self.vm.u.mem_write(self.params, struct.pack("<10f", 0, 3, *([0] * 8)))
        self.call(0xA0, self.params)
        self.call(0x98, 1)
        self.call(0xA8, 0)
        self.call(0x88, 0)
        self.call(0x48, 0)
        self.call(0x160)


def geq_flags(instance):
    vm = instance.vm
    processor = vm.q(instance.outer + 0x58)
    geq = vm.q(processor + 0x48)
    return list(vm.u.mem_read(geq + 0x48, 2))


def trial(dll, profile, rate, switch):
    local = OriginalLocal(dll, rate, None)
    local.write(0, CAPACITY)
    prior = local.read(31)
    local.write(CAPACITY, 511)
    if switch:
        local.apply(profile)
    before_new = geq_flags(local)
    prior += local.read(CAPACITY)
    local.write(CAPACITY + 511, CAPACITY - 511)
    after = local.read(CAPACITY)
    assert local.inputs == [CAPACITY, CAPACITY]
    assert all(call == "0x64f40" for call in local.native_calls)
    local.check_text()
    return prior, after, len(local.native_calls), before_new


def run(dll, profile, rate):
    old_control, _, old_setup, _ = trial(dll, profile, rate, False)
    old_in_place, in_place, hot_setup, hot_geq = trial(dll, profile, rate, True)
    assert old_in_place == old_control
    assert (old_setup, hot_setup) == (1, 2)
    fresh = Original(dll, rate, profile)
    fresh_geq = geq_flags(fresh)
    assert hot_geq == [0, 0] and fresh_geq == [0, 1]
    fresh.write(CAPACITY, CAPACITY)
    fresh_pcm = fresh.read(CAPACITY)
    assert fresh.inputs == [CAPACITY]
    fresh.check_text()
    assert len(fresh_pcm) == len(in_place) and fresh_pcm != in_place
    return {
        "rate": rate,
        "pendingInput": 511,
        "alreadyProducedFrames": len(old_in_place) // 8,
        "queuedOldPcmSha256": hashlib.sha256(old_in_place).hexdigest(),
        "newFramesBoth": len(in_place) // 8,
        "hotGeqBypassPrevious": hot_geq,
        "freshGeqBypassPrevious": fresh_geq,
        "hotNewPcmSha256": hashlib.sha256(in_place).hexdigest(),
        "freshNewPcmSha256": hashlib.sha256(fresh_pcm).hexdigest(),
        "sameFrameDifferentSampleWords": sum(
            a != b for a, b in zip(struct.iter_unpack("<I", in_place), struct.iter_unpack("<I", fresh_pcm), strict=True)
        ),
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
    profile = json.loads(corpus.read_text())["profiles"]["neutralTone"]["aev2"]
    receipt = {"sdkSha256": SDK_SHA, "cases": [run(args.dll, profile, rate) for rate in (44100, 48000)]}
    with args.out.open("x") as output:
        output.write(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
