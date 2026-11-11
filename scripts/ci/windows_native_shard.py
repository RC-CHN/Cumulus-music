"""Run one partition of Cargo's complete Windows all-target test inventory."""

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys


SHARDS = ("a", "b", "c")
# Balance timed core suites across runner shards. Hash the rest so newly
# discovered targets always get an owner. Run 43066560555 timed relay_publication
# at 81.19s on b, which completed 7m50s after a.
HEAVY = {
    "a": frozenset(("download_directory", "client_cloud", "completed_downloads",
                    "cumulus_core", "client_relay_publication", "cloud_upload", "relay_publication")),
    "b": frozenset(("pc_experiment_transport", "client_player", "client_fm",
                    "effects_service", "client_podcast")),
    "c": frozenset(("client_downloads", "client_together", "history", "cloud_download",
                    "download_service", "playback_controller")),
}


def owner(target):
    if target[0] == "core" and target[1].startswith("deps/"):
        name = target[1].removeprefix("deps/")
        for shard, names in HEAVY.items():
            if name in names:
                return shard
    return SHARDS[hashlib.sha256("\0".join(target).encode()).digest()[0] % len(SHARDS)]


def provenance():
    return [os.environ[name] for name in ("GITHUB_SHA", "GITHUB_RUN_ID", "GITHUB_RUN_ATTEMPT")]


def inventory_key(executable):
    root = Path(os.environ["GITHUB_WORKSPACE"]).resolve()
    relative = Path(executable).resolve(strict=True).relative_to(root)
    working_dir = Path.cwd().resolve().relative_to(root).as_posix()
    if working_dir not in ("core", "core/audio-filter", "desktop"):
        raise ValueError(f"unexpected Cargo working directory: {working_dir}")
    if relative.parts[:3] != (".build", "target", "debug") or len(relative.parts) != 5:
        raise ValueError(f"unexpected Cargo test executable: {relative}")
    name = re.fullmatch(r"([A-Za-z0-9_-]+)-[0-9a-f]{16}(?:\.exe)?", relative.name)
    if not name:
        raise ValueError(f"expected a hashed Cargo test executable: {relative.name}")
    return working_dir, f"{relative.parts[3]}/{name[1]}"


def run(executable, args):
    shard = os.environ["CUMULUS_NATIVE_SHARD"]
    if shard not in SHARDS:
        raise ValueError(f"unknown Windows Rust shard: {shard}")
    target = inventory_key(executable)
    selected = owner(target) == shard
    status = subprocess.run([executable, *args], check=False).returncode if selected else None
    with Path(os.environ["CUMULUS_NATIVE_INVENTORY"]).open("a", encoding="utf-8") as output:
        output.write(json.dumps([target, shard, status, provenance()]) + "\n")
    return status or 0


def verify(receipts):
    if len(receipts) != len(SHARDS):
        raise ValueError("expected exactly one Windows Rust inventory per shard")
    inventories = {}
    current_run = provenance()
    for receipt, shard in zip(receipts, SHARDS, strict=True):
        entries = {}
        for line in Path(receipt).read_text(encoding="utf-8").splitlines():
            target, recorded_shard, status, recorded_run = json.loads(line)
            if (not isinstance(target, list) or len(target) != 2 or
                    not all(isinstance(piece, str) and piece for piece in target)):
                raise ValueError(f"invalid target in Windows Rust shard {shard}: {target}")
            key = tuple(target)
            expected = 0 if owner(key) == shard else None
            if (key in entries or shard != recorded_shard or current_run != recorded_run or
                    status != expected or (status is not None and type(status) is not int)):
                raise ValueError(f"invalid or unsuccessful result for {key} on Windows Rust shard {shard}")
            entries[key] = status
        if not entries or all(status is None for status in entries.values()):
            raise ValueError(f"empty Windows Rust shard {shard}")
        inventories[shard] = entries
    first = inventories[SHARDS[0]].keys()
    for shard in SHARDS[1:]:
        targets = inventories[shard].keys()
        if targets != first:
            raise ValueError(f"Windows Cargo test inventories differ on {shard}: {sorted(first ^ targets)}")
    print(f"Verified {len(first)} Cargo test targets across Windows Rust shards: " + ", ".join(
        f"{shard}={sum(status is not None for status in inventories[shard].values())}" for shard in SHARDS
    ))


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "--verify":
        verify(sys.argv[2:])
    elif len(sys.argv) >= 2:
        sys.exit(run(sys.argv[1], sys.argv[2:]))
    else:
        sys.exit("missing Cargo test executable or --verify")
