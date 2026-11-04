"""Prove passive Windows IPv4 registration and terminal cancellation, without changing the network."""

import argparse
import json
import os
from pathlib import Path
import platform
import struct
import subprocess

from windows_nla_runtime import (
    OTHER_NETWORK_DLLS, fixed_network_imports, run_local_probe, unique_pairs,
)

SCOPE = {
    "nativeWindowsIpv4Watch": True,
    "addressDataRead": False,
    "networkConfigurationChanged": False,
    "playbackEnabled": False,
}
ROUND_KEYS = {"pendingArm", "exclusive", "beforeClose", "cancelAccepted", "terminal"}
IP_HELPER = {"CancelIPChangeNotify", "NotifyAddrChange"}
OTHER_IMPORTS = (OTHER_NETWORK_DLLS - {"iphlpapi.dll"}) | {"ws2_32.dll"}


def inspect_binary(binary):
    return fixed_network_imports(binary, "iphlpapi.dll", IP_HELPER, OTHER_IMPORTS, "IP Helper")


def cancelled(round_):
    return (
        round_ is not None
        and round_["pendingArm"] is True
        and round_["exclusive"] is True
        and round_["beforeClose"] == "idle"
        and round_["cancelAccepted"] is True
        and round_["terminal"] == "cancelled"
    )


def validate_round(round_):
    if not isinstance(round_, dict) or set(round_) != ROUND_KEYS:
        raise ValueError("unexpected lifecycle fields")
    if type(round_["pendingArm"]) is not bool:
        raise ValueError("invalid pending-arm marker")
    if not round_["pendingArm"]:
        if any(round_[key] is not None for key in ROUND_KEYS - {"pendingArm"}):
            raise ValueError("unarmed probe reported activity")
        return
    if type(round_["exclusive"]) is not bool:
        raise ValueError("invalid exclusivity marker")
    if round_["beforeClose"] not in ("idle", "changed", "cancelled", "error"):
        raise ValueError("unknown pre-close status")
    if round_["terminal"] not in ("unarmed", "changed", "cancelled", "retained"):
        raise ValueError("unknown terminal status")
    if round_["terminal"] == "unarmed":
        if round_["cancelAccepted"] is not None:
            raise ValueError("unarmed notification reported cancellation")
    elif type(round_["cancelAccepted"]) is not bool:
        raise ValueError("invalid cancellation acknowledgement")


def categorical_report(stdout):
    if len(stdout) > 1024:
        raise ValueError("unexpected probe output length")
    report = json.loads(stdout, object_pairs_hook=unique_pairs)
    if not isinstance(report, dict) or set(report) != set(SCOPE) | {"first", "second"}:
        raise ValueError("unexpected probe fields")
    if any(report[key] is not expected for key, expected in SCOPE.items()):
        raise ValueError("unexpected probe scope")
    validate_round(report["first"])
    if cancelled(report["first"]):
        validate_round(report["second"])
    elif report["second"] is not None:
        raise ValueError("probe retried after an unconfirmed cancellation")
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt" or platform.machine().lower() not in {"amd64", "x86_64"}:
        parser.exit(1, "A native Windows x64 process is required.\n")
    try:
        binary = args.binary.resolve(strict=True)
        imports = inspect_binary(binary)
        result = run_local_probe(binary, "cumulus-ipv4-")
        if result.stderr:
            raise ValueError("unexpected probe diagnostics")
        report = categorical_report(result.stdout)
        report["ipHelperImports"] = imports
        report["ok"] = result.returncode == 0 and cancelled(report["first"]) and cancelled(report["second"])
        with args.report.open("x", encoding="ascii") as file:
            json.dump(report, file, indent=2)
            file.write("\n")
    except (OSError, KeyError, ValueError, struct.error, subprocess.TimeoutExpired):
        # Never preserve raw child output, network settings or machine paths.
        parser.exit(1, "The IPv4 proof did not produce a bounded, categorical result.\n")
    if not report["ok"]:
        parser.exit(1, "Windows did not confirm both pending-arm/cancel lifecycles.\n")
    print("Windows confirmed both passive pending-arm/cancel lifecycles.")


if __name__ == "__main__":
    main()
