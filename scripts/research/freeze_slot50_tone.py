#!/usr/bin/env python3
"""Manually verify or refreeze synthetic slot-50 hashes using a local DLL.

Reuses the reviewed bounded SDK interpreter; never imports the DLL natively.
No original code, parameter blobs, audio samples, or host state are emitted.
"""

import argparse
from itertools import product
import json
from pathlib import Path
import platform
import struct
import subprocess
import sys

from freeze_full_stereo_material import equal, isolated, sha

CAPACITY = 2048
ACTIONS = [
    ("fresh_device_reset", CAPACITY),
    ("material_enable", CAPACITY),
    ("same_controls", CAPACITY),
    ("material_hot", CAPACITY),
    ("master_dry", 7),
    ("master_stays_dry", CAPACITY),
    ("full_reapply", CAPACITY),
    ("master_dry_then_reset", 7),
    ("full_reapply_then_reset", 7),
    ("material_disable", 7),
    ("off_stays_dry", 0),
]
FIXTURE = (Path(__file__).resolve().parents[2] / "core/tests/fixtures"
           / "effects-slot50-tone-official.json")


def synthetic_pcm(index, frames, channels):
    values = [
        (((frame * 13 + index * 7 + channel * 19) % 113) - 56) * 2**-10
        if frame < frames else 0
        for channel in range(channels)
        for frame in range(CAPACITY)
    ]
    if frames:
        values[0] = 0.5
        if channels == 2:
            values[CAPACITY] = -0.375
    return struct.pack(f"<{len(values)}f", *values)


def original_case(dll, outer_format, rate, channels):
    from full_stereo_oracle.vm import BASE, VM
    from unicorn.x86_const import UC_X86_REG_XMM1, UC_X86_REG_XMM2

    vm = VM(dll)
    for entry in (0x1C04E0, 0x19F208, 0x1A0508):
        assert vm.run(entry, 0) & 0xff == 1
    outer = vm.run(0x65B60, 0)
    assert vm.q(outer) == BASE + 0x23F8C8
    assert vm.run(0x670E0, outer, outer_format, channels, rate, 0) == 1
    assert [vm.i(outer + field) for field in (0x80, 0x84, 0x88)] == [CAPACITY, channels, rate]
    processor = vm.q(outer + 0x58)
    assert vm.q(processor) == BASE + 0x1E42B0
    node = vm.q(processor + 0x50)
    assert vm.owned(node, 0x50)
    vtable = vm.q(node) - BASE
    assert vtable == 0x244160 and vm.im.slot(vtable, 0x10) == 0xA2970
    byte_count = channels * CAPACITY * 4
    pcm, frames_ptr, end_ptr = vm.allocate(byte_count), vm.allocate(4), vm.allocate(1)

    def call(entry, offset, *args):
        assert vm.im.slot(vm.q(outer) - BASE, offset) == entry
        vm.run(entry, outer, *args)

    def material(bass, treble):
        call(0x68000, 0x158)
        call(0x692E0, 0x98, 0)
        enabled = bool(bass or treble)
        if enabled:
            for reg, gain in ((UC_X86_REG_XMM1, bass), (UC_X86_REG_XMM2, treble)):
                vm.u.reg_write(reg, struct.unpack("<I", struct.pack("<f", gain))[0])
            call(0x686B0, 0xB0)
        call(0x68600, 0xA8, int(enabled))
        call(0x6AB30, 0x88, 0)
        call(0x6A490, 0x48, 0)
        call(0x65BC0, 0x160)

    blocks = []
    for index, (action, frames) in enumerate(ACTIONS):
        if action in ("material_enable", "full_reapply", "full_reapply_then_reset"):
            material(4, -3)
        if action == "material_hot":
            material(-2, 6)
        if action.startswith("master_dry"):
            call(0x68770, 0x30, 1)
        if action == "fresh_device_reset" or action.endswith("_then_reset"):
            call(0x67F70, 0x28)
        if action == "material_disable":
            material(0, 0)
        before = list(vm.u.mem_read(node + 0x48, 2))
        end = index >= len(ACTIONS) - 2
        source = synthetic_pcm(index, frames, channels)
        vm.u.mem_write(pcm, source)
        vm.puti(frames_ptr, frames)
        vm.u.mem_write(end_ptr, bytes([end]))
        vm.run(0xA2970, node, pcm, end_ptr, frames_ptr)
        assert vm.i(frames_ptr) == frames and vm.u.mem_read(end_ptr, 1)[0] == end
        blocks.append(dict(action=action, frames=frames, end=end, inputSha256=sha(source),
                           outputSha256=sha(vm.u.mem_read(pcm, byte_count)), before=before,
                           after=list(vm.u.mem_read(node + 0x48, 2))))
    assert bytes(vm.u.mem_read(BASE + 0x1000, 0x1E2000)) == vm.im.data[0x1000:0x1E3000]
    assert not any("NLS" in event for event in vm.import_events)
    return dict(outerFormatEnum=outer_format, rate=rate, channels=channels, blocks=blocks)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True, help="pinned PC x64 SDK image")
    parser.add_argument("--output", type=Path, help="write a new fixture instead of comparing")
    args = parser.parse_args()
    if platform.system() != "Linux" or platform.machine() != "x86_64" or not __debug__:
        parser.error("use Linux x86_64 and an unoptimized Python interpreter")
    if not isolated():
        result = subprocess.run(
            ["unshare", "-Urn", sys.executable, str(Path(__file__).resolve()), *sys.argv[1:]],
            check=False,
        )
        raise SystemExit(result.returncode)
    from full_stereo_oracle.image import DLL_SHA256

    result = dict(
        schema="cumulus-slot50-tone-official-v1", dllSha256=DLL_SHA256,
        nodeRva="a2970", nodeVtableRva="244160", capacity=CAPACITY,
        originalTextUnmodified=True, fullOriginalSdkGraph=False,
        originalOuterFifo=False, realWindowsExecution=False,
        cases=[original_case(args.dll, fmt, rate, channels)
               for fmt, rate, channels in product((3, 4), (44100, 48000), (1, 2))],
    )
    if args.output is not None:
        with args.output.open("x", encoding="utf-8") as output:
            output.write(json.dumps(result, separators=(",", ":")) + "\n")
    else:
        if not FIXTURE.is_file() or FIXTURE.stat().st_size > 64 * 1024:
            raise ValueError("expected bounded frozen synthetic fixture")
        equal(result, json.loads(FIXTURE.read_bytes()), "slot50")
    print("Original slot 50: 8 geometries, 88 leaf blocks")


if __name__ == "__main__":
    main()
