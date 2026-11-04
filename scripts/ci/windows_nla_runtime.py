"""Run the fixed local-only NLA probe and preserve only its categorical report."""

import argparse
import json
import os
from pathlib import Path
import platform
import struct
import subprocess
import tempfile

MAX_IMAGE = 8 * 1024 * 1024
WINSOCK = frozenset({
    "WSAStartup", "WSACleanup", "WSALookupServiceBeginW",
    "WSALookupServiceNextW", "WSAGetLastError", "WSALookupServiceEnd",
})
OTHER_NETWORK_DLLS = {
    "wsock32.dll", "mswsock.dll", "wininet.dll", "winhttp.dll", "urlmon.dll",
    "dnsapi.dll", "wlanapi.dll", "iphlpapi.dll", "rasapi32.dll",
}
REPORT_FACTS = {
    "actualWindowsNla": True,
    "applicationNetworkRequests": 0,
    "parsedProviderRecords": 0,
    "listenerEnabled": False,
    "skipEnabled": False,
}
# (official bool, startup, begin, next); the latter two can be skipped.
CASES = {
    None: (None, "error", None, None),
    "BeginFailed": (True, "ok", "error", None),
    "ProviderPresent": (True, "ok", "ok", "ok"),
    "ProviderResultTooSmall": (True, "ok", "ok", "error"),
    "NoMoreProviders": (False, "ok", "ok", "error"),
    "OtherNextFailure": (False, "ok", "ok", "error"),
}


def direct_winsock_imports(binary):
    # Read only PE metadata. This is an import-boundary check, not an OS
    # firewall or a claim about the Windows NLA service's own activity.
    with Path(binary).open("rb") as stream:
        image = stream.read(MAX_IMAGE + 1)
    if len(image) > MAX_IMAGE or image[:2] != b"MZ":
        raise ValueError("not a bounded PE image")
    nt, = struct.unpack_from("<I", image, 0x3C)
    if image[nt:nt + 4] != b"PE\0\0":
        raise ValueError("missing PE signature")
    machine, count = struct.unpack_from("<HH", image, nt + 4)
    size, = struct.unpack_from("<H", image, nt + 20)
    optional = nt + 24
    kind, = struct.unpack_from("<H", image, optional)
    directory_count, = struct.unpack_from("<I", image, optional + 108)
    if machine != 0x8664 or kind != 0x20B or not 1 <= count <= 32:
        raise ValueError("expected a Win64 executable")
    if directory_count < 14 or size < 224:
        raise ValueError("missing PE directory information")
    import_rva, import_size = struct.unpack_from("<II", image, optional + 120)
    delay_rva, delay_size = struct.unpack_from("<II", image, optional + 216)
    if not import_rva or not 40 <= import_size <= 1024 * 1024 or delay_rva or delay_size:
        raise ValueError("missing bounded imports or unexpected delay imports")
    sections = []
    for index in range(count):
        section = optional + size + index * 40
        rva, length, offset = struct.unpack_from("<III", image, section + 12)
        sections.append((rva, length, offset))

    def offset_at(rva, need):
        for start, length, offset in sections:
            delta = rva - start
            if 0 <= delta and delta + need <= length and offset + delta + need <= len(image):
                return offset + delta
        raise ValueError("invalid import address")

    def name_at(rva):
        start = offset_at(rva, 1)
        end = image.find(b"\0", start, start + 128)
        if end <= start:
            raise ValueError("missing import name")
        offset_at(rva, end - start + 1)
        return image[start:end].decode("ascii")

    found = None
    for index in range(min(64, import_size // 20)):
        descriptor = offset_at(import_rva + index * 20, 20)
        original, stamp, chain, name_rva, first = struct.unpack_from("<5I", image, descriptor)
        if not any((original, stamp, chain, name_rva, first)):
            if found == WINSOCK:
                return sorted(found)
            raise ValueError("unexpected WinSock imports")
        dll = name_at(name_rva).lower()
        if dll in OTHER_NETWORK_DLLS or dll.startswith("api-ms-win-net"):
            raise ValueError("unexpected networking library")
        if dll != "ws2_32.dll":
            continue
        if found is not None:
            raise ValueError("duplicate WinSock imports")
        found = set()
        for thunk in range(32):
            address = offset_at((original or first) + thunk * 8, 8)
            symbol, = struct.unpack_from("<Q", image, address)
            if not symbol:
                break
            if symbol >= 1 << 32:
                raise ValueError("unexpected WinSock ordinal import")
            name = name_at(symbol + 2)
            if name not in WINSOCK or name in found:
                raise ValueError("unexpected WinSock API")
            found.add(name)
        else:
            raise ValueError("unterminated WinSock imports")
    raise ValueError("unterminated PE import descriptors")


def unique_pairs(pairs):
    obj = {}
    for key, value in pairs:
        if key in obj:
            raise ValueError("duplicate probe field")
        obj[key] = value
    return obj


def categorical_report(stdout):
    if len(stdout) > 1024:
        raise ValueError("unexpected probe output length")
    report = json.loads(stdout, object_pairs_hook=unique_pairs)
    if not isinstance(report, dict) or set(report) != set(REPORT_FACTS) | {
        "sample", "officialOnline", "calls",
    }:
        raise ValueError("unexpected probe fields")
    for key, expected in REPORT_FACTS.items():
        if type(report[key]) is not type(expected) or report[key] != expected:
            raise ValueError("unexpected probe scope")
    calls = report["calls"]
    if not isinstance(calls, dict) or set(calls) != {
        "startup", "begin", "next", "getLastError", "end", "cleanup",
    }:
        raise ValueError("unexpected probe call fields")
    sample = report["sample"]
    if (sample is not None and not isinstance(sample, str)) or sample not in CASES:
        raise ValueError("unknown probe result")
    online, startup, begin, next_call = CASES[sample]
    if report["officialOnline"] is not online or any(
        calls[key] != expected for key, expected in (
            ("startup", startup), ("begin", begin), ("next", next_call),
        )
    ):
        raise ValueError("probe result conflicts with call outcomes")
    if calls["getLastError"] is not (next_call == "error"):
        raise ValueError("unexpected last-error read")
    for key, called in (("end", begin == "ok"), ("cleanup", startup == "ok")):
        if (called and calls[key] not in ("ok", "error")) or (not called and calls[key] is not None):
            raise ValueError("unexpected cleanup sequence")
    return report


def verified_runtime(report):
    return report["sample"] in {"ProviderPresent", "ProviderResultTooSmall", "NoMoreProviders"} and (
        report["calls"]["end"] == report["calls"]["cleanup"] == "ok"
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if os.name != "nt" or platform.machine().lower() not in {"amd64", "x86_64"}:
        parser.exit(1, "A native Windows x64 process is required.\n")
    try:
        binary = args.binary.resolve(strict=True)
        imports = direct_winsock_imports(binary)
        system_root = os.environ["SystemRoot"]
        env = {"SystemRoot": system_root, "WINDIR": system_root}
        with tempfile.TemporaryDirectory(prefix="cumulus-nla-") as workdir:
            result = subprocess.run(
                [str(binary)], cwd=workdir, env=env, stdin=subprocess.DEVNULL,
                capture_output=True, text=True, encoding="ascii", timeout=10, check=False,
            )
        if result.stderr:
            raise ValueError("unexpected probe diagnostics")
        report = categorical_report(result.stdout)
        report["winsockImports"] = imports
        report["ok"] = result.returncode == 0 and verified_runtime(report)
        with args.report.open("x", encoding="ascii") as file:
            json.dump(report, file, indent=2)
            file.write("\n")
    except (OSError, KeyError, ValueError, struct.error, subprocess.TimeoutExpired):
        # Keep malformed or unexpected child output out of the workflow logs.
        parser.exit(1, "The local NLA proof did not produce a bounded, categorical result.\n")
    if not report["ok"]:
        parser.exit(1, "The native NLA provider did not complete the expected call sequence.\n")
    print("Native NLA proof passed; only categorical results were preserved.")


if __name__ == "__main__":
    main()
