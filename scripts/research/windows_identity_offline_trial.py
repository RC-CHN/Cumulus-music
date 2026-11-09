"""Print only reviewed diagnostic states from four standalone Windows probes."""

import os
from pathlib import Path
import re
import subprocess
import sys


READ = r"(?:QueryFailed|NextFailed|TimedOut|NoObject|BadObjectCount|PropertyFailed|NotBstr|TooLong|Empty|Present)"
DETAIL = r"(?:None|Some\(-?[0-9]{1,10}\))"
CHECK = r"(?:Unavailable|EncodingUnavailable|EmptyAfterConversion|ShortBufferMismatch|CrtUnavailable|Filtered|Accepted)"
FIELD = rf"{READ}:{DETAIL}:(?:not_read|failed|empty|present)"
SHAPE = r"(?:Null|UnsupportedTag|NullArray|UnsupportedRank|UnsupportedElement|UnknownBounds|OutOfBounds|CheckedMetadata)"
WMI_ERROR = (
    r"(?:worker_unavailable|deadline|worker_failed|startup_failed "
    r"stage=(?:com|security|locator|local_namespace|proxy) hresult=0x[0-9a-f]{8})"
)
COUNT = r"[0-9]{1,2}"
PARSER = r"(?:None|Some\((?:Header|Size|Offset|Terminator|DecodeTail|MediaStatus|SourceTooLong|DestinationTooLong)\))"
COUNTS = (
    rf"empty={COUNT} words={COUNT} raw={COUNT} escaped={COUNT} malformed={COUNT} "
    rf"access_denied={COUNT} unsupported={COUNT} other_os_error={COUNT} first_parser_rejection={PARSER}"
)
STORAGE = (
    rf"drive_slots=16 opened={COUNT} absent={COUNT} access_denied={COUNT} other_open_error={COUNT}",
    rf"descriptor {COUNTS}",
    rf"media {COUNTS}",
)

# Each known exit code binds both streams and the entire number/order of lines.
RULES = {
    'board': {
        0: (False, (
            rf"board_probe=complete acp=[0-9]{{1,5}} thread_acp=(?:None|Some\([0-9]{{1,5}}\)) "
            rf"board=\(\({READ}, {DETAIL}\), {CHECK}\) "
            rf"bios=\(\({READ}, {DETAIL}\), (?:None|Some\({CHECK}\))\) "
            r"selected=(?:None|Some\((?:Board|Bios)\)) private_values_emitted=0",
        )),
        1: (True, (rf"board_probe={WMI_ERROR}",)),
    },
    'host': {
        0: (False, (
            rf"host_probe=complete acp=[0-9]{{1,5}} vendor={FIELD} name={FIELD} "
            rf"enclosure=\({READ}, {DETAIL}, (?:None|Some\(\({SHAPE}, [0-9]{{1,5}}\)\))\) "
            r"host_description_complete=false private_values_emitted=0",
        )),
        1: (True, (rf"host_probe={WMI_ERROR}",)),
    },
    'mac': {
        0: (False, (r"status=registry_ndis_available formatted_bytes=17",)),
        3: (False, (r"status=fallback_required reason=(?:no_registry_candidate|outer_filter_rejected)",)),
        4: (False, (r"status=inconclusive reason=(?:enumeration_limit|registry_key|description|device_name|driver_reply)",)),
    },
    'storage': {
        0: (False, (*STORAGE, r"storage_probe=observed_metadata_only")),
        2: (False, (*STORAGE, r"storage_probe=no_readable_serial")),
        3: (True, (r"storage_probe=(?:unavailable|timed_out_or_stopped)",)),
    },
}


def validated_lines(label: str, result: subprocess.CompletedProcess) -> list[str]:
    rule = RULES[label].get(result.returncode)
    if rule is None:
        raise ValueError('unrecognized result code')
    is_stderr, patterns = rule
    public, other = (result.stderr, result.stdout) if is_stderr else (result.stdout, result.stderr)
    if other or len(public) > 2048 or not public.endswith(b'\n'):
        raise ValueError('unexpected probe streams')
    lines = public.replace(b'\r\n', b'\n').decode('ascii').removesuffix('\n').split('\n')
    if len(lines) != len(patterns) or any(
        re.fullmatch(pattern, line) is None for pattern, line in zip(patterns, lines)
    ):
        raise ValueError('unrecognized output shape')
    return lines


def main() -> int:
    if os.name != 'nt' or len(sys.argv) != 1:
        print('Windows x86_64 and no arguments required.', file=sys.stderr)
        return 1
    folder = Path('.build').resolve()
    for label, binary, args in (
        ('board', 'identity-board/debug/cumulus-windows-board-identity-probe.exe', ()),
        ('host', 'identity-board/debug/windows-host-description-probe.exe', ()),
        ('mac', 'identity-mac/debug/windows-mac-probe.exe', ('--collect',)),
        ('storage', 'identity-storage/probe.exe', ('--offline',)),
    ):
        try:
            result = subprocess.run(
                [folder / binary, *args], capture_output=True, check=False, timeout=20,
            )
            lines = validated_lines(label, result)
        except (OSError, subprocess.TimeoutExpired, ValueError):
            # Neither an exception body nor raw unvalidated output belongs in CI.
            print(f'{label}: unable to verify the offline result format', file=sys.stderr)
            return 1
        for line in lines:
            print(f'{label}: {line}', flush=True)
    return 0


if __name__ == '__main__':
    sys.exit(main())
