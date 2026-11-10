#!/usr/bin/env python3
"""Freeze synthetic PCM receipts from the pinned x64 effects DLL, offline only."""
import argparse
import copy
import hashlib
import json
import socket
import struct
from pathlib import Path

from unicorn.x86_const import UC_X86_REG_R8, UC_X86_REG_XMM1, UC_X86_REG_XMM2

from full_stereo_oracle.vm import BASE, VM


SDK_SHA = "c59105e1072d7d7dbd2d2844ac9c170de7e7b41eb463dc56a11c9b37bafac6be"
CAPACITY = 2048
ROOT = Path(__file__).resolve().parents[2]


def synth(first, count):
    return b"".join(
        struct.pack(
            "<ff",
            0.875 if i % 27 == 0 else ((i * 37) % 256 - 128) / 512,
            -0.625 if i % 29 == 0 else ((i * 53) % 128 - 64) / 512,
        )
        for i in range(first, first + count)
    )


def bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


class Original:
    def __init__(self, dll, rate, profile):
        self.vm = vm = VM(dll)
        for entry in (0x1C04E0, 0x19F208, 0x1A0508):
            assert vm.run(entry, 0) & 255 == 1
        self.outer = vm.run(0x65B60, 0)
        assert vm.q(self.outer) == BASE + 0x23F8C8
        assert vm.run(0x670E0, self.outer, 3, 2, rate, 0) == 1
        assert [vm.i(self.outer + offset) for offset in (0x80, 0x84, 0x88, 0x8C)] == [
            CAPACITY, 2, rate, 4,
        ]
        proc = vm.q(self.outer + 0x58)
        assert vm.q(proc) == BASE + 0x1E42B0
        self.b0 = vm.q(proc + 0xB0)
        self.buf = vm.allocate(CAPACITY * 8)
        self.out = vm.allocate(CAPACITY * 8)
        self.count = vm.allocate(4)
        self.end = vm.allocate(1)
        self.params = vm.allocate(64)
        self.inputs, self.outputs, self.native_calls = [], [], []
        self.frame_count = None
        self.apply(profile)

        def before():
            assert self.frame_count is None
            self.frame_count = vm.reg(UC_X86_REG_R8)
            self.inputs.append(vm.i(self.frame_count))

        def after():
            self.outputs.append(vm.i(self.frame_count))
            self.frame_count = None

        vm.hook(BASE + 0x1AA50, before)
        vm.hook(BASE + 0x67DDE, after)
        for entry in (0x1B960, 0x65490, 0x64F40):
            vm.hook(BASE + entry, lambda entry=entry: self.native_calls.append(hex(entry)))

    def call(self, slot, *args):
        vm = self.vm
        entry = vm.im.slot(vm.q(self.outer) - BASE, slot)
        return vm.run(entry, self.outer, *args)

    def apply(self, profile):
        assert set(profile) == {"eq", "bt", "peq", "limiter", "se", "rvb", "cmp"}
        assert all(
            value["on"] is (key in ("eq", "bt", "peq", "limiter"))
            for key, value in profile.items()
        )
        vm = self.vm
        self.call(0x158)  # Begin defaults
        vm.u.mem_write(self.params, struct.pack("<10f", *profile["eq"]["eqs"]))
        self.call(0xA0, self.params)
        self.call(0x98, 1)
        vm.u.reg_write(UC_X86_REG_XMM1, bits(profile["bt"]["bass"]))
        vm.u.reg_write(UC_X86_REG_XMM2, bits(profile["bt"]["treble"]))
        self.call(0xB0)
        self.call(0xA8, 1)
        self.call(0x88, 0)
        self.call(0x48, 0)
        peq = profile["peq"]
        vm.u.reg_write(UC_X86_REG_XMM1, bits(peq["gain"]))
        self.call(0xE8)
        for band in peq["f"]:
            vm.u.mem_write(self.params, struct.pack(
                "<IfffB3x", band["type"], band["gain"], band["freq"], band["q"], band["on"],
            ))
            self.call(0xF0, band["band"], self.params)
        self.call(0xE0, 1)
        self.call(0xC0, 0)
        limit = profile["limiter"]
        vm.u.mem_write(self.params, struct.pack(
            "<ffffB3xf", limit["threshold"], limit["attack"], limit["release"],
            limit["outputgain"], limit["infiniteRatio"], limit["ratio"],
        ))
        self.call(0x150, self.params)
        self.call(0x148, 1)
        self.call(0x160)

    def disable_master(self):
        self.vm.run(0x68770, self.outer, 1)

    def flags(self):
        return list(self.vm.u.mem_read(self.b0 + 0xC4, 2))

    def write(self, first, count):
        assert 0 <= count <= CAPACITY
        if count:
            self.vm.u.mem_write(self.buf, synth(first, count))
        self.call(0x18, self.buf, count * 8, 0xFFFFFFFF)
        assert self.frame_count is None

    def read(self, count, end=False):
        self.vm.puti(self.count, count * 8)
        self.vm.u.mem_write(self.end, b"\xa5")
        self.call(0x20, self.out, self.count, self.end)
        n = self.vm.i(self.count)
        assert n <= count * 8 and n % 8 == 0
        assert self.vm.u.mem_read(self.end, 1) == bytes([end])
        return bytes(self.vm.u.mem_read(self.out, n))

    def check_text(self):
        vm = self.vm
        assert bytes(vm.u.mem_read(BASE + 0x1000, 0x1E2000)) == vm.im.data[0x1000:0x1E3000]
        assert not any("NLS" in event for event in vm.import_events)


def freeze(dll, profile_name, profile, rate, mode):
    old = Original(dll, rate, profile)
    old.write(0, 2048)
    before = old.read(31)
    old.write(2048, 511)
    first = old.flags()
    if mode != "steady":
        old.disable_master()
    off = old.flags()
    before += old.read(2048)
    if mode == "coalesced":
        old.apply(profile)
    old.write(2559, 1537)
    middle = old.read(31)
    old.write(4096, 257)
    if mode == "switch":
        old.apply(profile)
    reapplied = old.flags()
    middle += old.read(2048)
    old.write(4353, 1791)
    tail = old.read(2048)
    old.write(6144, 7)
    old.write(6151, 0)
    tail += old.read(2048, end=True)
    assert old.read(2048, end=True) == b""
    assert old.inputs == [2048, 2048, 2048, 7]
    assert all(call == "0x64f40" for call in old.native_calls)
    old.check_text()
    pcm = (before, middle, tail)
    return {
        "profile": profile_name, "rate": rate, "mode": mode,
        "inputs": old.inputs, "outputs": old.outputs,
        "frames": [len(item) // 8 for item in pcm],
        "sha256": [hashlib.sha256(item).hexdigest() for item in pcm],
        "flags": [first, off, reapplied], "nativeB0Calls": old.native_calls,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    assert {name for _, name in socket.if_nameindex()} <= {"lo"}, "use a loopback-only namespace"
    assert args.dll.is_file() and args.dll.stat().st_size <= 8 * 1024 * 1024, "expected bounded local DLL"
    assert hashlib.sha256(args.dll.read_bytes()).hexdigest() == SDK_SHA, "unexpected effects DLL"
    corpus = ROOT / "core/tests/fixtures/effects-modern-limiter-graph-official.json"
    profiles = json.loads(corpus.read_text())["profiles"]
    profiles["defaultTiming"] = copy.deepcopy(profiles["nonzeroTone"])
    profiles["defaultTiming"]["aev2"]["limiter"].update(
        attack=0.005, release=0.1, threshold=0, outputgain=0,
    )
    profiles["releaseOnlyTiming"] = copy.deepcopy(profiles["nonzeroTone"])
    profiles["releaseOnlyTiming"]["aev2"]["limiter"]["attack"] = 0.005
    cases = [
        freeze(args.dll, name, profile["aev2"], rate, mode)
        for name, profile in profiles.items()
        for rate in (44100, 48000)
        for mode in ("steady", "switch", "coalesced")
    ]
    result = {"sdkSha256": SDK_SHA, "cases": cases, "schema": "cumulus.modern-limiter-x64-hot.v1"}
    with args.out.open("x") as target:
        target.write(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
