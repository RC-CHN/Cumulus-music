"""Run a read-only Windows default audio endpoint sample and save categories only."""

import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess

from windows_nla_runtime import run_local_probe, unique_pairs

LOOKUP = "find_default_endpoint"
E_NOTFOUND = "0x80070490"
BEFORE_ENDPOINT = {"initialize_com", "create_enumerator", LOOKUP}
AFTER_ENDPOINT = {"activate_volume", "get_scalar", "get_mute"}


def categorical_report(stdout):
    if len(stdout) > 256:
        raise ValueError("unexpected probe output length")
    result = json.loads(stdout, object_pairs_hook=unique_pairs)
    if not isinstance(result, dict) or set(result) != {"kind", "stage", "hresult"}:
        raise ValueError("unexpected probe output fields")
    stage, code = result["stage"], result["hresult"]
    if stage is None and code is None:
        expected = "endpoint_readable"
    elif stage == "invalid_scalar" and code is None:
        expected = "endpoint_unreadable"
    elif isinstance(stage, str) and isinstance(code, str) and re.fullmatch(r"0x[89a-f][0-9a-f]{7}", code):
        if stage == LOOKUP and code == E_NOTFOUND:
            expected = "default_endpoint_absent"
        elif stage in BEFORE_ENDPOINT:
            expected = "backend_unknown"
        elif stage in AFTER_ENDPOINT:
            expected = "endpoint_unreadable"
        else:
            raise ValueError("unknown probe stage")
    else:
        raise ValueError("invalid probe stage/code pair")
    if result["kind"] != expected:
        raise ValueError("probe category conflicts with call outcome")
    return result


def collect_sample(binary):
    try:
        probe = run_local_probe(binary, "cumulus-system-master-volume-")
    except subprocess.TimeoutExpired:
        return dict(kind="backend_unknown", stage="probe_timeout", hresult=None, probeReturned=False)
    if probe.stderr or probe.returncode:
        raise ValueError("unexpected probe process result")
    return {**categorical_report(probe.stdout), "probeReturned": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    if os.name != "nt" or platform.machine().lower() not in {"amd64", "x86_64"}:
        parser.exit(1, "A native Windows x64 process is required.\n")
    if not re.fullmatch(r"[0-9a-f]{40}", args.commit):
        parser.exit(1, "An exact checked-out source SHA is required.\n")
    try:
        report = {
            "commit": args.commit,
            "runtime": "native_windows_x64",
            "dataFlow": "render",
            "role": "multimedia",
            **collect_sample(args.binary.resolve(strict=True)),
        }
        with args.report.open("x", encoding="ascii") as file:
            json.dump(report, file, indent=2)
            file.write("\n")
    except (OSError, KeyError, ValueError):
        # Do not echo unexpected child stdout/stderr, COM messages or paths.
        parser.exit(1, "The local sample did not produce a valid categorical report.\n")
    print(f"Windows system master volume sample: {report['kind']} ({report['stage'] or 'complete'})")


if __name__ == "__main__":
    main()
