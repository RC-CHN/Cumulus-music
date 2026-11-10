#!/usr/bin/env python3
"""Execute sealed clock excerpts from a caller-supplied original x64 image.

The actual instructions, including their RIP-relative constants, are mapped
only in Unicorn. Mock SDK/COM calls and scratch buffers never access a
Windows device, account, media path, or network endpoint.
"""

import argparse
import hashlib
import json
from pathlib import Path
import socket
import struct

import pefile
from unicorn import (
    UC_ARCH_X86,
    UC_HOOK_CODE,
    UC_MODE_64,
    UC_PROT_EXEC,
    UC_PROT_READ,
    Uc,
)
from unicorn.x86_const import (
    UC_X86_REG_R8,
    UC_X86_REG_R9,
    UC_X86_REG_RAX,
    UC_X86_REG_RBP,
    UC_X86_REG_RCX,
    UC_X86_REG_RDX,
    UC_X86_REG_RIP,
    UC_X86_REG_RSP,
)

HASH = "557d7706dedc8556a8bd279d4dfe2bc05b80ad729a8749caccd9e29916935493"


def probe(dll: Path) -> dict:
    if not __debug__ or {name for _, name in socket.if_nameindex()} - {"lo"}:
        raise ValueError("use a loopback-only namespace with Python assertions enabled")
    if not dll.is_file() or not 20 * 1024**2 < dll.stat().st_size < 64 * 1024**2:
        raise ValueError("expected a bounded original cloudmusic.dll")
    raw = dll.read_bytes()
    if hashlib.sha256(raw).hexdigest() != HASH:
        raise ValueError("unexpected cloudmusic.dll digest")
    image = pefile.PE(data=raw, fast_load=True)
    if (
        image.FILE_HEADER.Machine != 0x8664
        or image.OPTIONAL_HEADER.ImageBase != 0x180000000
    ):
        raise ValueError("unexpected image architecture/base")
    U = Uc(UC_ARCH_X86, UC_MODE_64)
    base = image.OPTIONAL_HEADER.ImageBase
    binary = image.get_memory_mapped_image()
    mapped_size = (len(binary) + 4095) & ~4095
    U.mem_map(base, mapped_size)
    U.mem_write(base, binary)
    U.mem_protect(base, mapped_size, UC_PROT_READ | UC_PROT_EXEC)
    U.mem_map(0x100000, 0x80000)
    BP = 0x110000
    SP = 0x150000
    OBJ = 0x130000
    DEST = 0x132000
    STATE = 0x134000
    VT = 0x136000
    PCM = 0x138000
    PCM_OUT = 0x140000
    EXTERNAL = 0x160000
    SDK = 0x162000
    SDK_VT = 0x164000

    def p(address, fmt, *values):
        U.mem_write(address, struct.pack("<" + fmt, *values))

    def g(address, fmt):
        return struct.unpack(
            "<" + fmt, U.mem_read(address, struct.calcsize("<" + fmt))
        )[0]

    reg = U.reg_read
    w = U.reg_write

    def run(start, end):
        w(UC_X86_REG_RBP, BP)
        U.emu_start(base + start, base + end, count=4096)
        assert reg(UC_X86_REG_RIP) == base + end

    def backend_count(rate, frame_bytes, frames, speed):
        p(OBJ + 0x13C, "H", frame_bytes)
        p(OBJ + 0xF0, "I", rate)
        p(BP + 0xEF0, "Q", OBJ)
        p(BP + 0xF00, "I", frames * frame_bytes)
        p(BP + 0xF08, "d", speed)
        run(0x10920E0, 0x1092199)
        return g(OBJ + 0x1C0, "Q"), g(OBJ + 0x1B8, "Q")

    def getter(state):
        p(BP + 0xE0, "Q", OBJ)
        w(UC_X86_REG_RAX, state)
        run(0x108F436, 0x108F44D)
        return reg(UC_X86_REG_RAX)

    def reported(source_ms, clip_seconds, device_us):
        p(BP + 0x48, "Q", source_ms)
        w(UC_X86_REG_RAX, device_us)
        run(0x1014C53, 0x1014C73)
        composed_ms = g(BP + 0x28, "Q")
        w(UC_X86_REG_RAX, composed_ms)
        p(BP + 0x280, "Q", STATE)
        p(BP + 0x288, "Q", DEST)
        p(STATE + 0x98, "d", clip_seconds)
        run(0x10174AE, 0x10174FE)
        return composed_ms, g(DEST, "d")

    def seek_raw(seconds):
        p(BP + 0x120, "Q", STATE)
        p(BP + 0x128, "d", seconds)
        run(0x10B00DF, 0x10B0125)
        assert g(STATE + 0x40, "I") == 1
        return g(STATE + 0x48, "Q")

    def clear_counter():
        p(BP + 0xF0, "Q", OBJ)
        run(0x10925A5, 0x10925B7)
        return g(OBJ + 0x1C0, "Q"), g(OBJ + 0x1B8, "Q")

    calls = []

    def stub(addr, fn):
        def call(uc, *_):
            r = fn()
            sp = reg(UC_X86_REG_RSP)
            ret = g(sp, "Q")
            w(UC_X86_REG_RSP, sp + 8)
            w(UC_X86_REG_RAX, r)
            w(UC_X86_REG_RIP, ret)

        U.hook_add(UC_HOOK_CODE, call, begin=addr, end=addr)

    def memcopy():
        n = reg(UC_X86_REG_R8)
        assert 0 < n <= 4096
        calls.append(["copy", n])
        U.mem_write(reg(UC_X86_REG_RCX), bytes(U.mem_read(reg(UC_X86_REG_RDX), n)))
        return reg(UC_X86_REG_RCX)

    def com_release():
        calls.append(["release", reg(UC_X86_REG_RDX), reg(UC_X86_REG_R8)])
        return 0

    stub(base + 0xC51540, memcopy)
    stub(base + 0x108CDB0, lambda: STATE)
    stub(EXTERNAL, com_release)
    p(STATE, "Q", VT)
    p(VT + 0x20, "Q", EXTERNAL)

    # Run actual non-null WASAPI write sub-branch. The COM buffer is synthetic,
    # initialized to A5 so untouched bytes are independently observable.
    period = 2048
    actual = 1829
    frame_bytes = 2
    U.mem_write(PCM, bytes([0x5A]) * (actual * frame_bytes))
    U.mem_write(PCM_OUT, bytes([0xA5]) * (period * frame_bytes))
    p(BP + 0x108, "Q", PCM_OUT)
    p(BP + 0xEF8, "Q", PCM)
    p(BP + 0x64, "I", actual)
    p(BP + 0xEF0, "Q", OBJ)
    p(BP + 0x84, "I", 0)
    p(OBJ + 0x13C, "H", frame_bytes)
    p(OBJ + 0x1B4, "I", period)
    w(UC_X86_REG_RSP, SP)
    run(0x1091E78, 0x1091F2A)
    assert calls == [["copy", actual * frame_bytes], ["release", period, 0]], calls
    assert bytes(U.mem_read(PCM_OUT, period * frame_bytes)) == bytes([0x5A]) * (
        actual * frame_bytes
    ) + bytes([0xA5]) * ((period - actual) * frame_bytes)
    commit = list(calls)
    calls.clear()
    p(BP + 0x108, "Q", 0)
    w(UC_X86_REG_RSP, SP)
    run(0x1091E78, 0x1091F2A)
    assert calls == [["release", 0, 2]], calls
    null_commit = list(calls)
    calls.clear()

    # Execute original cloudmusic.dll 1055EF0. Diagnostic debugger call and
    # unique PCM buffer getter are replaced; SDK slots are counted fixtures.
    stub(base + 0xC4D23C, lambda: 0)
    stub(base + 0x1056110, lambda: PCM)

    def sdk_write():
        assert reg(UC_X86_REG_RCX) == SDK and reg(UC_X86_REG_RDX) == PCM
        calls.append(["sdkWrite", reg(UC_X86_REG_R8), reg(UC_X86_REG_R9)])
        return 0

    def sdk_read():
        assert reg(UC_X86_REG_RCX) == SDK and reg(UC_X86_REG_RDX) == PCM
        assert reg(UC_X86_REG_R9) == OBJ + 0xE8
        n = reg(UC_X86_REG_R8)
        calls.append(["sdkReadCapacity", g(n, "I")])
        p(n, "I", actual * frame_bytes)
        p(OBJ + 0xE8, "B", 0)
        return 0

    stub(EXTERNAL + 8, sdk_write)
    stub(EXTERNAL + 16, sdk_read)
    p(SDK, "Q", SDK_VT)
    p(SDK_VT + 0x18, "Q", EXTERNAL + 8)
    p(SDK_VT + 0x20, "Q", EXTERNAL + 16)
    length = DEST
    stack = SP + 8
    stop = EXTERNAL + 24
    p(length, "I", period * frame_bytes)
    p(stack, "Q", stop)
    p(stack + 0x28, "Q", PCM)
    p(stack + 0x30, "I", period * frame_bytes)
    p(stack + 0x38, "Q", length)
    w(UC_X86_REG_RSP, stack)
    w(UC_X86_REG_RCX, OBJ)
    w(UC_X86_REG_RDX, SDK)
    w(UC_X86_REG_R8, PCM)
    w(UC_X86_REG_R9, length)
    U.emu_start(base + 0x1055EF0, stop, count=4096)
    assert reg(UC_X86_REG_RIP) == stop and g(length, "I") == actual * frame_bytes
    assert calls == [
        ["sdkWrite", period * frame_bytes, 0xFFFFFFFF],
        ["sdkReadCapacity", period * frame_bytes],
    ], calls
    boundary = list(calls)
    calls.clear()

    cases = []
    for rate, normal, hot in (
        (44100, [1829, 2048, 2048, 226], [1829, 1829, 1829, 226]),
        (48000, [1810, 2048, 2048, 245], [1810, 1810, 1810, 245]),
    ):
        outputs = []
        for label, blocks in (("continuous", normal), ("offApply", hot)):
            p(OBJ + 0x1C0, "Q", 0)
            p(OBJ + 0x1B8, "Q", 0)
            counts = [backend_count(rate, 8, n, 1.0) for n in blocks]
            device_us = counts[-1][1]
            assert (
                device_us == sum(blocks) * 1_000_000 // rate
                and getter(1) == device_us
                and getter(3) == 0
            )
            base_ms = seek_raw(3.375)
            assert base_ms == 3375
            observed = reported(base_ms, 0.375, device_us)
            assert observed == (
                3375 + device_us // 1000,
                (3375 + device_us // 1000) / 1000 - 0.375,
            )
            previous = clear_counter()
            assert previous == (0, device_us) and getter(1) == device_us
            first_after = backend_count(rate, 8, 13, 1.25)
            assert (
                first_after == (130, 16 * 1_000_000 // rate)
                and getter(1) == first_after[1]
            )
            after = reported(base_ms, 0.375, first_after[1])
            assert after[0] == 3375
            clamp = reported(0, 0.375, 0)
            assert clamp == (0, 0.0)
            outputs.append(
                dict(
                    name=label,
                    blocks=blocks,
                    counts=counts,
                    seekBaseMs=base_ms,
                    progressBeforeSeek=observed,
                    retainedAfterBackendResetUs=previous[1],
                    firstAfterSeek=first_after,
                    progressAfterFirstWrite=after,
                    zeroClamp=clamp,
                )
            )
        cases.append(dict(rate=rate, cases=outputs))

    p(OBJ + 0x1C0, "Q", 0)
    p(OBJ + 0x1B8, "Q", 0)
    eight = [backend_count(44100, 4, 1, 1.125) for _ in range(8)]
    p(OBJ + 0x1C0, "Q", 0)
    p(OBJ + 0x1B8, "Q", 0)
    one = backend_count(44100, 4, 8, 1.125)
    assert eight[-1] == (32, 181) and one == (36, 204)
    report = dict(
        dllSha256=HASH,
        nativeExcerpts=True,
        deviceRuntime=False,
        sdkCalls=boundary,
        sdkReadBytes=actual * frame_bytes,
        mockWasapiCommit=commit,
        mockWasapiNull=null_commit,
        cases=cases,
        countedRounding={
            "rate": 44100,
            "frameBytes": 4,
            "speed": 1.125,
            "eightWrites": eight,
            "oneWrite": one,
        },
    )
    return report


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dll", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    report = probe(args.dll)
    with args.out.open("x", encoding="utf-8") as output:
        json.dump(report, output, indent=2)
        output.write("\n")
