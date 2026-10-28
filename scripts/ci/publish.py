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


def find_release(repository, tag):
    # The tags endpoint does not return an unpublished draft. Resolve its ID.
    pages = api(f"repos/{repository}/releases?per_page=100", "--paginate", "--slurp")
    matches = [release for page in pages for release in page if release["tag_name"] == tag]
    assert len(matches) <= 1, "duplicate release records"
    if not matches:
        return None
    release = api(f"repos/{repository}/releases/{matches[0]['id']}")
    assert release["id"] == matches[0]["id"] and release["tag_name"] == tag, "release identity changed"
    return release


def validate_tag(reference, tag, sha):
    assert reference["ref"] == f"refs/tags/{tag}", "wrong tag"
    assert reference["object"]["type"] == "commit" and reference["object"]["sha"] == sha, "tag target changed"


def validate_uploaded(release, tag, files):
    assert release["tag_name"] == tag and release["draft"] is True and not release["prerelease"], "only the exact unpublished draft can be resumed"
    assets = release["assets"]
    expected = {path.name: path for path in files}
    assert len(assets) == len(expected) and {asset["name"] for asset in assets} == set(expected), "uploaded asset set mismatch"
    for asset in assets:
        path = expected[asset["name"]]
        assert asset["state"] == "uploaded" and asset["size"] == path.stat().st_size, "uploaded asset size/state mismatch"
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        assert asset.get("digest") == f"sha256:{digest}", "uploaded asset digest missing or mismatch"


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
    parser.add_argument("--resume-draft", action="store_true", help="verify an existing tag/draft without creating or replacing assets")
    args = parser.parse_args()
    assert VERSION.fullmatch(args.version) and args.run_id.isdecimal(), "invalid release input"
    repository = os.environ["GITHUB_REPOSITORY"]
    assert os.environ["GITHUB_REF"] == "refs/heads/main", "CD must run on main"
    assert git("rev-parse", "HEAD") == os.environ["GITHUB_SHA"], "checkout differs from CD commit"
    tag = f"v{args.version}"
    if args.resume_draft:
        reference = api(f"repos/{repository}/git/ref/tags/{tag}")
        sha = check_release(args.version, reference["object"]["sha"])
        validate_tag(reference, tag, sha)
        # Recovery code may follow the marker on main, but cannot publish a
        # release from a different lineage or substitute its own source tree.
        subprocess.run(["git", "merge-base", "--is-ancestor", sha, "HEAD"], check=True)
        draft = find_release(repository, tag)
        assert draft is not None and draft["draft"] is True, "unpublished draft missing"
    else:
        sha = check_release(args.version)
        assert sha == os.environ["GITHUB_SHA"], "release must target the CD commit"
        draft = None
    workflow = api(f"repos/{repository}/actions/workflows/ci.yml")

    def verified_run():
        run = api(f"repos/{repository}/actions/runs/{args.run_id}")
        validate_run(run, repository, sha, args.run_id, workflow["id"])
        jobs = api(f"repos/{repository}/actions/runs/{args.run_id}/attempts/{run['run_attempt']}/jobs?per_page=100")["jobs"]
        successes = {job["name"] for job in jobs if job["conclusion"] == "success" and job["status"] == "completed"}
        assert REQUIRED_JOBS <= successes, "required CI jobs have not all succeeded"
        return run

    run = verified_run()
    if not args.resume_draft:
        existing = api(f"repos/{repository}/git/matching-refs/tags/{tag}")
        assert not any(ref["ref"] == f"refs/tags/{tag}" for ref in existing), "release tag already exists; never move or overwrite it"
        assert find_release(repository, tag) is None, "release already exists (including draft)"
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
    uploaded_files = [*files, sums, evidence]
    if args.resume_draft:
        validate_uploaded(draft, tag, uploaded_files)
    # Recheck after download; a rerun must not substitute another attempt.
    assert verified_run()["run_attempt"] == run["run_attempt"], "CI was rerun during release preparation"
    if args.publish:
        if not args.resume_draft:
            # No force option: a racing/preexisting tag causes an API failure.
            api(f"repos/{repository}/git/refs", "--method", "POST", "-f", f"ref=refs/tags/{tag}", "-f", f"sha={sha}")
            # Upload into a draft; a failed upload never exposes a partial release.
            subprocess.run(["gh", "release", "create", tag, "--repo", repository, "--verify-tag", "--draft", "--title", f"Cumulus Music {args.version}", "--notes-file", str(notes), *map(str, uploaded_files)], check=True)
        release = find_release(repository, tag)
        assert release is not None, "uploaded draft missing"
        if draft is not None:
            assert release["id"] == draft["id"], "draft replaced during recovery"
        validate_uploaded(release, tag, uploaded_files)
        validate_tag(api(f"repos/{repository}/git/ref/tags/{tag}"), tag, sha)
        assert verified_run()["run_attempt"] == run["run_attempt"], "CI was rerun before publication"
        published = api(f"repos/{repository}/releases/{release['id']}", "--method", "PATCH", "-F", "draft=false", "-f", "make_latest=true")
        assert published["id"] == release["id"] and published["tag_name"] == tag and published["draft"] is False, "publication was not confirmed"
    print(json.dumps({"ok": True, "published": args.publish, "resumedDraft": args.resume_draft, "tag": tag, "commit": sha, "ciRun": run["id"]}))


if __name__ == "__main__":
    main()
