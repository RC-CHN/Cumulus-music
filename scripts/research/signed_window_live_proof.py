#!/usr/bin/env python3
"""Explicit native signed-window proof launcher; retains only sanitized evidence."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile

from signed_window_broker import Broker, PC_DIALS, Pin, verify_certificate

STAGES = ["ux_account_resources", "ux_search_song", "ux_playing", "ux_paused", "ux_resumed", "ux_stopped"]
DISCOVERY_STAGES = STAGES[:2]
# Native profile preparation still expects this field; discovery never pins it.
DISCOVERY_PROFILE_HOST = "m1.music.126.net"
MAX_REPLY = 16384


class Refused(ValueError):
    pass


def require(condition):
    if not condition:
        raise Refused("signed window proof requirements not met")


def checked_file(path, *, private=False, digest_sealed_copy=False):
    """Caller must use a canonical path; open the exact no-follow final entry."""
    require(path.is_absolute() and path.resolve(strict=True) == path)
    fd = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        info = os.fstat(fd)
        require(stat.S_ISREG(info.st_mode) and info.st_uid == os.geteuid()
                and (digest_sealed_copy or info.st_mode & 0o022 == 0))
        if private:
            require(info.st_mode & 0o077 == 0 and info.st_nlink == 1)
        return os.fdopen(fd, "rb")
    except BaseException:
        os.close(fd)
        raise


def read_private_json(path, limit):
    with checked_file(path, private=True) as stream:
        content = stream.read(limit + 1)
    require(len(content) <= limit)
    return json.loads(content)


def seal(source, destination, expected):
    require(re.fullmatch(r"[0-9a-f]{64}", expected))
    digest = hashlib.sha256()
    # Cargo's output may be group-writable. Only the separately digest-checked
    # private copy is ever executed or bound into the sandbox.
    with checked_file(source, digest_sealed_copy=True) as existing, destination.open("xb") as copied:
        require(os.fstat(existing.fileno()).st_mode & 0o100)
        for chunk in iter(lambda: existing.read(1024 * 1024), b""):
            digest.update(chunk)
            copied.write(chunk)
    require(digest.hexdigest() == expected)
    destination.chmod(0o500)


def count(value, minimum, maximum):
    return type(value) is int and minimum <= value <= maximum


def accepted_report(report, assets_sha, *, discover_cdn=False):
    """Rebuild evidence from a fixed set of primitive facts, never raw fields."""
    require(isinstance(report, dict))
    positives = ("ok", "liveReadOnly", "productionWebView", "isolatedNetwork",
                 "searchAfterResources")
    stages = DISCOVERY_STAGES if discover_cdn else STAGES
    media_count = 0 if discover_cdn else 1
    if discover_cdn:
        positives += ("discoveryOnly",)
        try:
            # Pin.cdn is a syntax-only constructor. This address is never dialed.
            media_authority = Pin.cdn(report.get("mediaAuthority"), "1.1.1.1").host.decode("ascii")
        except ValueError:
            raise Refused("invalid discovered CDN authority") from None
        mode_facts = {"mediaAuthority": media_authority}
    else:
        require(report.get("discoveryOnly", False) is False)
        positives += ("pauseHeldSameEntry", "resumedSameEntry", "stopped",
                      "fullTimeline", "observedLibmpvNullClock")
        clocks = report.get("playbackClocksMs")
        require(isinstance(clocks, dict) and set(clocks) == {"playing", "paused", "resumed"}
                and all(count(clock, 100, 120000) for clock in clocks.values())
                and clocks["resumed"] > clocks["paused"] >= clocks["playing"])
        mode_facts = {"playbackClocksMs": clocks.copy()}
    require(all(report.get(key) is True for key in positives)
            and report.get("physicalAudioOutput") is False
            and report.get("stages") == stages
            and count(report.get("mediaReads"), media_count, media_count)
            and count(report.get("auxiliaryAttempts"), 0, 0)
            and count(report.get("otherMediaAttempts"), 0, 0)
            and count(report.get("anonymousPinnedLoads"), media_count, media_count))
    assets = report.get("assets")
    require(isinstance(assets, dict) and assets.get("embedded") is True
            and assets.get("sha256") == assets_sha and count(assets.get("entries"), 1, 10000))
    gate = report.get("gate")
    gate_checks = ("expectedAccountVerified", "comprehensiveRead", "selectedSongFound",
                   "songDetailRead", "mediaAuthorized", "mediaConsumed")
    require(isinstance(gate, dict) and all(gate.get(key) is True for key in gate_checks)
            and gate.get("closedAfterFailure") is False
            and type(gate.get("searchV3")) is bool and type(gate.get("searchTabRead")) is bool
            and (not gate["searchV3"] or gate["searchTabRead"])
            and count(gate.get("playlistPages"), 1, 16)
            and count(gate.get("playlistCount"), 0, 3200)
            and count(gate.get("likedCount"), 0, 1000000)
            and count(gate.get("attemptedReads"), 8, PC_DIALS)
            and count(gate.get("deniedBeforeDispatch"), 0, 10000))
    return {**{key: True for key in positives}, **mode_facts, "physicalAudioOutput": False,
            "stages": stages.copy(), "mediaReads": media_count, "anonymousPinnedLoads": media_count,
            "auxiliaryAttempts": 0, "otherMediaAttempts": 0,
            "assets": {"embedded": True, "entries": assets["entries"], "sha256": assets_sha},
            "gate": {key: gate[key] for key in (*gate_checks, "searchV3", "searchTabRead",
                     "playlistPages", "playlistCount", "likedCount", "attemptedReads",
                     "deniedBeforeDispatch", "closedAfterFailure")}}


def rejected_checkpoint(output):
    """Keep bounded facts from a failed window; never retain the child report."""
    try:
        report = read_private_json(output / "report.json", MAX_REPLY)
    except (OSError, ValueError):
        return {}
    if not isinstance(report, dict) or report.get("liveReadOnly") is not True:
        return {}
    facts = {}
    stages = report.get("stages")
    if isinstance(stages, list) and stages == STAGES[:len(stages)]:
        facts["completedStages"] = len(stages)
    for key in ("initialized", "searchAfterResources", "observedLibmpvNullClock"):
        if type(report.get(key)) is bool:
            facts[key] = report[key]
    for key in ("mediaReads", "anonymousPinnedLoads", "auxiliaryAttempts", "otherMediaAttempts"):
        if count(report.get(key), 0, 10000):
            facts[key] = report[key]
    gate = report.get("gate")
    if isinstance(gate, dict):
        booleans = ("expectedAccountVerified", "comprehensiveRead", "selectedSongFound",
                    "songDetailRead", "mediaAuthorized", "mediaConsumed", "searchV3",
                    "searchTabRead", "closedAfterFailure")
        bounded = ("playlistPages", "playlistCount", "likedCount", "attemptedReads",
                   "deniedBeforeDispatch")
        facts["gate"] = {key: gate[key] for key in booleans if type(gate.get(key)) is bool}
        facts["gate"].update({key: gate[key] for key in bounded
                              if count(gate.get(key), 0, 1000000)})
    return facts


def write_evidence(root, result):
    fd = os.open(root / "result.json", os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as stream:
        json.dump(result, stream, ensure_ascii=True, indent=2)
        stream.write("\n")


def run(args):
    require(args.authorize_readonly and args.source_stopped)
    # Import only at invocation; standalone validation tests need no sandbox.
    from signed_window_sandbox import run as isolated_window

    require(re.fullmatch(r"[0-9a-f]{64}", args.assets_sha256))
    root = args.output
    require(root.is_absolute() and root.parent.resolve(strict=True) == root.parent)
    root.mkdir(mode=0o700)
    stage = "validate_intent"
    failure = {}
    try:
        intent = read_private_json(args.intent, 4096)
        require((args.cdn_ipv4 is None) == args.discover_cdn)
        fields = {"keyword", "songId"} if args.discover_cdn else {"keyword", "songId", "mediaHost"}
        require(isinstance(intent, dict) and set(intent) == fields)
        keyword, song = intent["keyword"], intent["songId"]
        require(isinstance(keyword, str) and 1 <= len(keyword) <= 64 and keyword.strip() == keyword
                and all(character.isprintable() for character in keyword)
                and isinstance(song, str) and re.fullmatch(r"[1-9][0-9]{0,19}", song))
        with checked_file(args.expected_uid_file, private=True):
            pass  # Only the sealed native helper reads and compares the actual UID.
        require(args.source.is_absolute() and args.source.resolve(strict=True) == args.source
                and args.source.is_dir())
        pc = Pin.pc(args.pc_ipv4)
        cdn = None if args.discover_cdn else Pin.cdn(intent["mediaHost"], args.cdn_ipv4)
        with tempfile.TemporaryDirectory(prefix="csw-") as directory:
            scratch = Path(directory).resolve(strict=True)
            stage = "seal_binaries"
            helper, probe = scratch / "prepare", scratch / "probe"
            seal(args.profile_helper, helper, args.profile_helper_sha256)
            seal(args.probe, probe, args.probe_sha256)

            # Verify each independently reviewed numeric pin before copying any credentials.
            stage = "verify_pins"
            verify_certificate(pc)
            if cdn is not None:
                verify_certificate(cdn)
            stage = "prepare_profile"
            prepared = scratch / "prepared"
            # Snapshot the reviewed intent. The helper's discovery placeholder is inert.
            intent_copy = scratch / "intent.json"
            fd = os.open(intent_copy, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC, 0o600)
            with os.fdopen(fd, "w", encoding="utf-8") as stream:
                json.dump({**intent, "mediaHost": DISCOVERY_PROFILE_HOST} if args.discover_cdn else intent,
                          stream, ensure_ascii=True)
            command = [helper, "--prepare", args.source, args.expected_uid_file, intent_copy, prepared]
            if args.derive_if_missing:
                command.append("--derive-if-missing")
            result = subprocess.run(command, env={"PATH": "/usr/bin:/usr/sbin", "LANG": "en_US.UTF-8"},
                                    stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=subprocess.DEVNULL, timeout=10, check=False)
            require(result.returncode == 0 and len(result.stdout) <= 128)
            preparation = json.loads(result.stdout)
            require(isinstance(preparation, dict) and set(preparation) == {"ok", "identityWasDerived"}
                    and preparation["ok"] is True and type(preparation["identityWasDerived"]) is bool
                    and (not preparation["identityWasDerived"] or args.derive_if_missing))

            work, output, relay = (scratch / name for name in ("work", "output", "relay"))
            for path in (work, output, relay):
                path.mkdir(mode=0o700)
            stage = "isolated_window"
            broker = Broker(relay, pc, cdn)
            try:
                with broker:
                    if args.discover_cdn:
                        sandbox = isolated_window(probe, prepared, work, output, broker.path, discover_cdn=True)
                    else:
                        sandbox = isolated_window(probe, prepared, work, output, broker.path)
            finally:
                failure["broker"] = broker.summary()
                checkpoint = rejected_checkpoint(output)
                if checkpoint:
                    failure["checkpoint"] = checkpoint
            stage = "verify_evidence"
            network = broker.summary()
            expected_media_dials = 0 if args.discover_cdn else 1
            require(network["disarmed"] is True and count(network["pcDialAttempts"], 1, PC_DIALS)
                    and count(network["mediaDialAttempts"], expected_media_dials, expected_media_dials)
                    and count(network["deniedConnections"], 0, 0))
            isolation_flags = ("loopbackOnly", "privateSessionBus", "privateDisplay", "privateBroker")
            expected_bridges = network["pcDialAttempts"] + network["mediaDialAttempts"]
            require(isinstance(sandbox, dict) and all(sandbox.get(key) is True for key in isolation_flags)
                    and count(sandbox.get("mainRoutes"), 0, 0) and count(sandbox.get("probeExit"), 0, 0)
                    and count(sandbox.get("bridgeConnections"), expected_bridges, expected_bridges))
            evidence = accepted_report(read_private_json(output / "report.json", MAX_REPLY), args.assets_sha256,
                                       discover_cdn=args.discover_cdn)
            evidence["sandbox"] = {**{key: True for key in isolation_flags}, "mainRoutes": 0,
                                   "probeExit": 0, "bridgeConnections": expected_bridges}
            evidence["broker"] = network
            evidence["probeSha256"] = args.probe_sha256
            evidence["profileHelperSha256"] = args.profile_helper_sha256
            evidence["identityWasDerived"] = preparation["identityWasDerived"]
        evidence["temporaryProfileRemoved"] = not scratch.exists()
        require(evidence["temporaryProfileRemoved"])
        write_evidence(root, evidence)
    except (ValueError, RuntimeError, OSError, subprocess.SubprocessError):
        # Errors can contain a local path, host, subprocess argv, or input data.
        write_evidence(root, {"ok": False, "stage": stage, **failure})
        raise Refused("signed window proof stopped") from None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for path in ("probe", "profile-helper", "source", "expected-uid-file", "intent", "output"):
        parser.add_argument("--" + path, type=Path, required=True)
    for value in ("probe-sha256", "profile-helper-sha256", "assets-sha256", "pc-ipv4"):
        parser.add_argument("--" + value, required=True)
    parser.add_argument("--cdn-ipv4", help="reviewed CDN pin, required for playback and prohibited for discovery")
    parser.add_argument("--discover-cdn", action="store_true", help="read one authorized media hostname without fetching audio")
    for option in ("authorize-readonly", "source-stopped", "derive-if-missing"):
        parser.add_argument("--" + option, action="store_true")
    try:
        run(parser.parse_args())
    except (ValueError, RuntimeError, OSError, ImportError):
        raise SystemExit("signed window proof unavailable; inspect the sanitized result if present") from None


if __name__ == "__main__":
    main()
