#!/usr/bin/env python3
"""Verify a synthetic fingerprint resampler report against the frozen DLL oracle."""

import argparse
import hashlib
import json
from pathlib import Path


FIXTURE = (
    Path(__file__).resolve().parents[2]
    / "core/tests/fixtures/song-recognition-resampler-official.json"
)
FILES = {
    "inputSha256": "input.bin",
    "monoSha256": "mono.bin",
    "outputSha256": "output.bin",
    "diagnosticTailSha256": "tail.bin",
    "packetsSha256": "packets.csv",
}


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def verify(fixture, log, prefix, mode):
    info, *rows = (json.loads(line) for line in log.read_text().splitlines())
    if info["version"] != fixture["swresampleVersion"]:
        raise ValueError("unexpected swresample ABI version")
    if mode == "scalar" and info["cpuFlags"] != 0:
        raise ValueError("scalar mode did not disable CPU acceleration")
    expected_cases = fixture["cases"]
    if len(rows) != len(expected_cases):
        raise ValueError("unexpected number of probe cases")
    for actual, row in zip(rows, expected_cases, strict=True):
        case = row["case"]
        if actual.pop("name") != case:
            raise ValueError(f"unexpected case/order: {case}")
        expected = row[mode]
        actual |= {
            key: sha256(Path(f"{prefix}-{case}-{suffix}"))
            for key, suffix in FILES.items()
        }
        for field in expected:
            if actual.get(field) != expected[field]:
                raise ValueError(f"{case} {mode} differs at {field}")
    return len(rows), info["cpuFlags"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("prefix", type=Path, help="same output prefix as the C probe")
    parser.add_argument("mode", choices=("auto", "scalar"))
    args = parser.parse_args()
    count, flags = verify(json.loads(FIXTURE.read_text()), args.log, args.prefix, args.mode)
    print(f"{count} exact synthetic cases, {args.mode}, CPU flags {flags:#x}")


if __name__ == "__main__":
    main()
