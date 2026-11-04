#!/usr/bin/env python3
"""Read the native AB key initializer in official PC 3.1.42.205594 x64."""

import argparse
import hashlib
import json
import struct
from pathlib import Path


DLL_SHA256 = "557d7706dedc8556a8bd279d4dfe2bc05b80ad729a8749caccd9e29916935493"


def extract_keys(image):
    if hashlib.sha256(image).hexdigest() != DLL_SHA256:
        raise ValueError("expected the version-pinned official x64 cloudmusic.dll")

    pe, = struct.unpack_from("<I", image, 0x3C)
    section_count, = struct.unpack_from("<H", image, pe + 6)
    optional_size, = struct.unpack_from("<H", image, pe + 20)
    optional = pe + 24
    image_base, = struct.unpack_from("<Q", image, optional + 24)
    table = optional + optional_size
    sections = [
        struct.unpack_from("<IIII", image, table + index * 40 + 8)
        for index in range(section_count)
    ]

    def file_offset(rva, length):
        for virtual_size, start, raw_size, raw_offset in sections:
            if start <= rva and rva + length <= start + min(virtual_size, raw_size):
                return raw_offset + (rva - start)
        raise ValueError(f"RVA 0x{rva:X} is not backed by section bytes")

    def read(rva, length):
        start = file_offset(rva, length)
        return image[start : start + length]

    keys = []
    for index in range(25):
        source_rva = 0x01D9BD + index * 0x14
        code = read(source_rva, 0x14)
        source_op = code[:3]
        if source_op not in (b"\x48\x8b\x15", b"\x48\x8d\x15"):
            raise ValueError(f"unexpected argument instruction at RVA 0x{source_rva:X}")
        if code[7:10] != b"\x48\x8d\x8d" or code[14] != 0xE8 or code[19] != 0x90:
            raise ValueError(f"unexpected string constructor sequence at RVA 0x{source_rva:X}")
        source_rel, = struct.unpack_from("<i", code, 3)
        stack_offset, = struct.unpack_from("<i", code, 10)
        call_rel, = struct.unpack_from("<i", code, 15)
        if stack_offset != 0x210 + index * 0x20 or source_rva + 19 + call_rel != 0x070840:
            raise ValueError(f"unexpected destination or constructor at RVA 0x{source_rva:X}")
        key_rva = source_rva + 7 + source_rel
        if source_op == b"\x48\x8b\x15":
            key_va, = struct.unpack("<Q", read(key_rva, 8))
            key_rva = key_va - image_base
        key_bytes = read(key_rva, 64)
        key, nul, _ = key_bytes.partition(b"\0")
        if not key or not nul:
            raise ValueError(f"missing bounded key at RVA 0x{key_rva:X}")
        keys.append(key.decode("ascii"))
    return keys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dll", type=Path)
    args = parser.parse_args()
    try:
        keys = extract_keys(args.dll.read_bytes())
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    print(json.dumps(keys, indent=2))


if __name__ == "__main__":
    main()
