"""Local, fail-closed release rules shared by CI and CD (no network)."""
import argparse
import json
from pathlib import Path
import re
import subprocess
import tomllib

VERSION = re.compile(r"(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\.(?:0|[1-9]\d*)\Z")
SUBJECT = re.compile(r"chore:release (.+)\Z")


def git(*args):
    return subprocess.check_output(["git", *args], text=True, encoding="utf-8").strip()


def check_versions(root=Path("."), expected=None):
    cargo = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    version = cargo["workspace"]["package"]["version"]
    assert VERSION.fullmatch(version), "release version must be X.Y.Z"
    assert expected is None or version == expected, "requested version differs from workspace"
    for file in ["frontend/package.json", "frontend/package-lock.json", "desktop/tauri.conf.json"]:
        data = json.loads((root / file).read_text(encoding="utf-8"))
        assert data["version"] == version, f"version mismatch: {file}"
        if file.endswith("package-lock.json"):
            assert data["packages"][""]["version"] == version, "lock root version mismatch"
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    for package in lock["package"]:
        if package["name"] in {"cumulus-core", "cumulus-desktop", "cumulus-audio-filter"}:
            assert package["version"] == version, "Cargo.lock workspace version mismatch"
    assert (root / f"docs/releases/{version}.txt").is_file(), "release notes missing"
    return version


def check_history(lines):
    versions = {}
    for line in lines:
        sha, subject = line.split(" ", 1)
        match = SUBJECT.fullmatch(subject)
        if match:
            version = match[1]
            assert VERSION.fullmatch(version), f"invalid release subject at {sha}"
            assert version not in versions or versions[version] == sha, f"duplicate release commit: {version}"
            versions[version] = sha
    return versions


def check_release(version, sha="HEAD"):
    check_versions(expected=version)
    sha = git("rev-parse", f"{sha}^{{commit}}")
    assert git("show", "-s", "--format=%s", sha) == f"chore:release {version}", "tag target is not the release commit"
    releases = check_history(git("log", "--all", "--format=%H %s").splitlines())
    assert releases.get(version) == sha, "release commit is not unique in fetched history"
    parents = git("rev-list", "--parents", "-n", "1", sha).split()
    assert len(parents) == 2, "release commit must have exactly one parent"
    assert git("rev-parse", f"{sha}^{{tree}}") == git("rev-parse", f"{sha}^^{{tree}}"), "prepare changes before the release commit; release marker must have the tested tree"
    return sha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release")
    args = parser.parse_args()
    version = check_versions(expected=args.release)
    check_history(git("log", "--all", "--format=%H %s").splitlines())
    if args.release:
        check_release(args.release)
    # Check every existing version tag, not only the one being published.
    for tag in git("tag", "--list", "v*").splitlines():
        assert VERSION.fullmatch(tag[1:]), f"unexpected release tag: {tag}"
        assert git("show", "-s", "--format=%s", f"{tag}^{{commit}}") == f"chore:release {tag[1:]}", f"wrong release target: {tag}"
    print(json.dumps({"ok": True, "version": version, "release": bool(args.release)}))


if __name__ == "__main__":
    main()
