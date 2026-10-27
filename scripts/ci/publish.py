"""Publish only artifacts from a successful CI run of the unique release commit."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from release_guard import VERSION, check_release, git

PLATFORMS = ("linux-x64", "windows-x64")
REQUIRED_JOBS = {"Frontend and release rules", "Native (linux-x64)", "Native (windows-x64)", "CI gate"}


def api(path, *args):
    return json.loads(subprocess.check_output(["gh", "api", path, *args], text=True, encoding="utf-8"))


def validate_run(run, repository, sha, run_id, workflow_id):
    assert str(run["id"]) == str(run_id), "wrong CI run"
    assert run["repository"]["full_name"] == repository and run["head_repository"]["full_name"] == repository, "CI repository mismatch"
    assert run["workflow_id"] == workflow_id and run["path"] == ".github/workflows/ci.yml", "wrong workflow"
    assert run["event"] == "push" and run["head_branch"] == "main", "only a main push CI is publishable"
    assert run["head_sha"] == sha, "CI must test the tag's exact commit"
    assert run["status"] == "completed" and run["conclusion"] == "success", "full CI must succeed before releasing"


def validate_manifest(directory, version, sha, platform, run_id, attempt):
    manifest_file = directory / f"manifest-{platform}.json"
    manifest = json.loads(manifest_file.read_text(encoding="utf-8"))
    assert manifest["version"] == version and manifest["commit"] == sha and manifest["platform"] == platform, "artifact provenance mismatch"
    assert str(manifest["ciRun"]) == str(run_id) and str(manifest["ciAttempt"]) == str(attempt), "artifact is from a different CI attempt"
    assert manifest["nativePlayback"]["ok"] is True and manifest["nativePlayback"]["remote_requests"] == 0, "packaged media verification missing"
    prefix = f"Cumulus-{version}-{platform}"
    expected = {prefix + ".deb", prefix + ".tar.gz"} if platform == "linux-x64" else {prefix + ".zip", prefix + "-setup.exe"}
    assert {row["name"] for row in manifest["assets"]} == expected and len(manifest["assets"]) == 2, "unexpected release assets"
    assert {p.name for p in directory.iterdir()} == expected | {manifest_file.name}, "extra artifact contents"
    paths = []
    for row in manifest["assets"]:
        path = directory / row["name"]
        assert path.is_file() and not path.is_symlink() and path.stat().st_size == row["bytes"], "artifact size/type mismatch"
        with path.open("rb") as stream:
            assert hashlib.file_digest(stream, "sha256").hexdigest() == row["sha256"], "artifact digest mismatch"
        paths.append(path)
    return [*paths, manifest_file]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--publish", action="store_true", help="otherwise verify/download only")
    args = parser.parse_args()
    assert VERSION.fullmatch(args.version) and args.run_id.isdecimal(), "invalid release input"
    repository = os.environ["GITHUB_REPOSITORY"]
    assert os.environ["GITHUB_REF"] == "refs/heads/main", "CD must run on main"
    sha = check_release(args.version)
    assert sha == os.environ["GITHUB_SHA"], "checkout differs from CD commit"
    workflow = api(f"repos/{repository}/actions/workflows/ci.yml")

    def verified_run():
        run = api(f"repos/{repository}/actions/runs/{args.run_id}")
        validate_run(run, repository, sha, args.run_id, workflow["id"])
        jobs = api(f"repos/{repository}/actions/runs/{args.run_id}/attempts/{run['run_attempt']}/jobs?per_page=100")["jobs"]
        successes = {job["name"] for job in jobs if job["conclusion"] == "success" and job["status"] == "completed"}
        assert REQUIRED_JOBS <= successes, "required CI jobs have not all succeeded"
        return run

    run = verified_run()
    tag = f"v{args.version}"
    existing = api(f"repos/{repository}/git/matching-refs/tags/{tag}")
    assert not any(ref["ref"] == f"refs/tags/{tag}" for ref in existing), "release tag already exists; never move or overwrite it"
    releases = json.loads(subprocess.check_output(["gh", "release", "list", "--repo", repository, "--limit", "1000", "--json", "tagName"], text=True))
    assert not any(release["tagName"] == tag for release in releases), "release already exists (including draft)"
    destination = Path(".build/cd")
    destination.mkdir(parents=True, exist_ok=False)
    files = []
    for platform in PLATFORMS:
        directory = destination / platform
        subprocess.run(["gh", "run", "download", args.run_id, "--repo", repository, "--name", f"packages-{platform}", "--dir", str(directory)], check=True)
        files.extend(validate_manifest(directory, args.version, sha, platform, args.run_id, run["run_attempt"]))
    sums = destination / "SHA256SUMS"
    lines = []
    for path in sorted(files):
        with path.open("rb") as stream:
            lines.append(f"{hashlib.file_digest(stream, 'sha256').hexdigest()}  {path.name}\n")
    sums.write_text("".join(lines), encoding="utf-8")
    evidence = destination / "release-provenance.json"
    evidence.write_text(json.dumps({"version": args.version, "tag": tag, "commit": sha,
        "ciRun": run["id"], "ciAttempt": run["run_attempt"], "ciUrl": run["html_url"],
        "workflow": run["path"], "allRequiredJobsSucceeded": True,
        "rebuiltInCD": False}, indent=2) + "\n", encoding="utf-8")
    notes = destination / "release-notes.txt"
    notes.write_text(Path(f"docs/releases/{args.version}.txt").read_text(encoding="utf-8") + f"\nCI: {run['html_url']}\nCommit: {sha}\n", encoding="utf-8")
    # Recheck after download; a rerun must not substitute another attempt.
    assert verified_run()["run_attempt"] == run["run_attempt"], "CI was rerun during release preparation"
    if args.publish:
        # No force option: a racing/preexisting tag causes an API failure.
        api(f"repos/{repository}/git/refs", "--method", "POST", "-f", f"ref=refs/tags/{tag}", "-f", f"sha={sha}")
        # Upload into a draft; a failed upload never exposes a partial release.
        subprocess.run(["gh", "release", "create", tag, "--repo", repository, "--verify-tag", "--draft", "--title", f"Cumulus Music {args.version}", "--notes-file", str(notes), *map(str, [*files, sums, evidence])], check=True)
        release = api(f"repos/{repository}/releases/tags/{tag}")
        assert {asset["name"] for asset in release["assets"]} == {p.name for p in [*files, sums, evidence]}, "uploaded asset set mismatch"
        assert verified_run()["run_attempt"] == run["run_attempt"], "CI was rerun before publication"
        subprocess.run(["gh", "release", "edit", tag, "--repo", repository, "--draft=false", "--latest"], check=True)
    print(json.dumps({"ok": True, "published": args.publish, "tag": tag, "commit": sha, "ciRun": run["id"]}))


if __name__ == "__main__":
    main()
