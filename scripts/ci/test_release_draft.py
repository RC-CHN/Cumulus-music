import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import publish


class DraftValidation(unittest.TestCase):
    def test_authenticated_draft_lookup_uses_exact_tag_and_release_id(self):
        draft = {"id": 91, "tag_name": "v0.1.0", "draft": True}
        with patch("publish.api", side_effect=[
            [[{"id": 90, "tag_name": "v0.1.01"}], [draft]], draft
        ]) as api:
            self.assertEqual(publish.find_release("owner/repo", "v0.1.0"), draft)
        self.assertEqual(api.call_args_list[1].args, ("repos/owner/repo/releases/91",))
        with patch("publish.api", return_value=[[]]):
            self.assertIsNone(publish.find_release("owner/repo", "v0.1.0"))
        with patch("publish.api", return_value=[[draft], [draft]]):
            with self.assertRaisesRegex(AssertionError, "duplicate"):
                publish.find_release("owner/repo", "v0.1.0")

    def test_lookup_rejects_release_identity_change(self):
        with patch("publish.api", side_effect=[
            [[{"id": 91, "tag_name": "v0.1.0"}]],
            {"id": 92, "tag_name": "v0.1.0"}
        ]):
            with self.assertRaisesRegex(AssertionError, "identity"):
                publish.find_release("owner/repo", "v0.1.0")

    def test_resume_never_accepts_published_or_modified_assets(self):
        with tempfile.TemporaryDirectory() as tmp:
            file = Path(tmp) / "synthetic-package"
            file.write_bytes(b"verified CI bytes")
            asset = {"name": file.name, "size": file.stat().st_size, "state": "uploaded",
                     "digest": "sha256:" + hashlib.sha256(file.read_bytes()).hexdigest()}
            draft = {"id": 91, "tag_name": "v0.1.0", "draft": True, "prerelease": False, "assets": [asset]}
            publish.validate_uploaded(draft, "v0.1.0", [file])
            for change in [
                {"draft": False}, {"prerelease": True}, {"tag_name": "v0.1.1"},
                {"assets": []}, {"assets": [asset, asset]},
                *({"assets": [dict(asset, **diff)]} for diff in [
                    {"digest": None}, {"digest": "sha256:" + "0" * 64},
                    {"size": 999}, {"state": "processing"}, {"name": "other"}
                ])
            ]:
                with self.subTest(change=change), self.assertRaises(AssertionError):
                    publish.validate_uploaded(dict(draft, **change), "v0.1.0", [file])

    def test_rc_resume_requires_a_matching_prerelease_draft(self):
        draft = {"id": 91, "tag_name": "v0.2.0-rc.1", "draft": True, "prerelease": True, "assets": []}
        publish.validate_uploaded(draft, "v0.2.0-rc.1", [])
        with self.assertRaisesRegex(AssertionError, "channel mismatch"):
            publish.validate_uploaded(draft | {"prerelease": False}, "v0.2.0-rc.1", [])
        with self.assertRaisesRegex(AssertionError, "unpublished draft"):
            publish.validate_uploaded(draft | {"draft": False}, "v0.2.0-rc.1", [])

    def test_tag_must_still_point_directly_to_the_verified_commit(self):
        tag = {"ref": "refs/tags/v0.1.0", "object": {"type": "commit", "sha": "verified"}}
        publish.validate_tag(tag, "v0.1.0", "verified")
        for change in [
            {"ref": "refs/tags/v0.1.1"},
            {"object": {"type": "commit", "sha": "changed"}},
            {"object": {"type": "tag", "sha": "verified"}},
        ]:
            with self.assertRaises(AssertionError):
                publish.validate_tag(dict(tag, **change), "v0.1.0", "verified")


class RequiredJobs(unittest.TestCase):
    def test_release_refuses_without_each_successful_linux_webview_job_before_package_download(self):
        repository = "owner/repo"
        sha = "release-sha"
        run = {"id": 12, "repository": {"full_name": repository},
               "head_repository": {"full_name": repository}, "workflow_id": 42,
               "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
               "head_sha": sha, "status": "completed", "conclusion": "success", "run_attempt": 1}
        for lane in ("Linux WebView smoke", "Linux WebView probe build",
                     "Linux WebView download, search and player smoke"):
            successful = [
                {"name": name, "status": "completed", "conclusion": "success"}
                for name in publish.REQUIRED_JOBS - {lane}
            ]
            for failed in ([], [{"name": lane, "status": "completed", "conclusion": "failure"}]):
                with self.subTest(lane=lane, failed=failed), \
                     patch.dict(os.environ, {"GITHUB_REPOSITORY": repository,
                                             "GITHUB_REF": "refs/heads/main", "GITHUB_SHA": sha}), \
                     patch("sys.argv", ["publish.py", "--version", "0.1.0", "--run-id", "12"]), \
                     patch("publish.git", return_value=sha), \
                     patch("publish.check_release", return_value=sha), \
                     patch("publish.api", side_effect=[{"id": 42}, run, {"jobs": successful + failed}]), \
                     patch("publish.subprocess.run") as command:
                    with self.assertRaisesRegex(AssertionError, "required CI jobs"):
                        publish.main()
                    command.assert_not_called()


class ResumeFlow(unittest.TestCase):
    def test_recovery_only_publishes_existing_draft_and_preserves_target_ci(self):
        for version, prerelease in (("0.1.0", False), ("0.2.0-rc.1", True)):
            with self.subTest(version=version):
                self.check_resume(version, prerelease)

    def check_resume(self, version, prerelease):
        tag = f"v{version}"
        previous = Path.cwd()
        with tempfile.TemporaryDirectory() as tmp:
            os.chdir(tmp)
            try:
                Path("docs/releases").mkdir(parents=True)
                Path(f"docs/releases/{version}.txt").write_text("Synthetic notes")
                draft = {"id": 91, "tag_name": tag, "draft": True, "prerelease": prerelease, "assets": []}
                run = {"id": 12, "repository": {"full_name": "owner/repo"},
                       "head_repository": {"full_name": "owner/repo"}, "workflow_id": 42,
                       "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
                       "head_sha": "release-sha", "status": "completed", "conclusion": "success",
                       "run_attempt": 1, "html_url": "https://example.invalid/ci/12"}
                calls = []

                def api(path, *args):
                    calls.append((path, args))
                    if path.endswith(f"/git/ref/tags/{tag}"):
                        return {"ref": f"refs/tags/{tag}", "object": {"type": "commit", "sha": "release-sha"}}
                    if path.endswith("/releases?per_page=100"):
                        return [[copy.deepcopy(draft)]]
                    if path.endswith("/releases/91"):
                        result = copy.deepcopy(draft)
                        if args:
                            channel = ("-F", "prerelease=true", "-f", "make_latest=false") if prerelease else (
                                "-f", "make_latest=true")
                            self.assertEqual(args, ("--method", "PATCH", "-F", "draft=false", *channel))
                            result["draft"] = False
                        return result
                    if path.endswith("/actions/workflows/ci.yml"):
                        return {"id": 42}
                    if path.endswith("/actions/runs/12"):
                        return run
                    if path.endswith("/attempts/1/jobs?per_page=100"):
                        return {"jobs": [{"name": name, "status": "completed", "conclusion": "success"} for name in publish.REQUIRED_JOBS]}
                    raise AssertionError(f"unexpected API: {path} {args}")

                def manifest(directory, expected, sha, platform, run_id, attempt):
                    self.assertEqual((expected, sha, run_id, attempt), (version, "release-sha", "12", 1))
                    directory.mkdir(parents=True)
                    file = directory / (platform + ".test")
                    file.write_bytes(b"synthetic CI artifact")
                    return [file]

                with patch.dict(os.environ, {"GITHUB_REPOSITORY": "owner/repo", "GITHUB_REF": "refs/heads/main", "GITHUB_SHA": "recovery-sha"}), \
                     patch("sys.argv", ["publish.py", "--version", version, "--run-id", "12", "--publish", "--resume-draft"]), \
                     patch("publish.git", return_value="recovery-sha"), \
                     patch("publish.check_release", return_value="release-sha") as marker, \
                     patch("publish.api", side_effect=api), \
                     patch("publish.subprocess.run") as command, \
                     patch("publish.validate_manifest", side_effect=manifest), \
                     patch("publish.validate_uploaded") as uploaded:
                    publish.main()
                marker.assert_called_once_with(version, "release-sha")
                self.assertEqual(uploaded.call_count, 2)
                commands = [call.args[0] for call in command.call_args_list]
                self.assertEqual(commands[0], ["git", "merge-base", "--is-ancestor", "release-sha", "HEAD"])
                self.assertEqual(len(commands), 3)  # ancestry and two CI artifact downloads
                self.assertTrue(all(command[:3] == ["gh", "run", "download"] for command in commands[1:]))
                self.assertEqual(sum("--method" in args for _, args in calls), 1)
                evidence = json.loads(Path(".build/cd/release-provenance.json").read_text())
                self.assertEqual(evidence["commit"], "release-sha")
                self.assertEqual(evidence["ciRun"], 12)
                self.assertFalse(evidence["rebuiltInCD"])
            finally:
                os.chdir(previous)


class ReleaseCandidateFlow(unittest.TestCase):
    def test_fresh_rc_creates_prerelease_draft_and_does_not_mark_latest(self):
        previous = Path.cwd()
        with tempfile.TemporaryDirectory() as tmp:
            os.chdir(tmp)
            try:
                self.check_rc_create()
            finally:
                os.chdir(previous)

    def check_rc_create(self):
        repository, version, sha = "owner/repo", "0.2.0-rc.1", "marker-sha"
        tag = f"v{version}"
        Path("docs/releases").mkdir(parents=True)
        Path(f"docs/releases/{version}.txt").write_text("Synthetic RC notes")
        run = {"id": 12, "repository": {"full_name": repository},
               "head_repository": {"full_name": repository}, "workflow_id": 42,
               "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
               "head_sha": sha, "status": "completed", "conclusion": "success",
               "run_attempt": 1, "html_url": "https://example.invalid/ci/12"}
        ref = {"ref": f"refs/tags/{tag}", "object": {"type": "commit", "sha": sha}}
        writes = []
        uploaded = []

        def api(path, *args):
            if path.endswith("/actions/workflows/ci.yml"):
                return {"id": 42}
            if path.endswith("/actions/runs/12"):
                return run
            if path.endswith("/attempts/1/jobs?per_page=100"):
                return {"jobs": [{"name": name, "status": "completed", "conclusion": "success"}
                                 for name in publish.REQUIRED_JOBS]}
            if path.endswith(f"/git/matching-refs/tags/{tag}"):
                return [{"ref": "refs/tags/v0.2.0-rc.10"}]
            if path.endswith(f"/git/ref/tags/{tag}"):
                return ref
            writes.append((path, args))
            if path.endswith("/git/refs"):
                self.assertEqual(args, ("--method", "POST", "-f", f"ref=refs/tags/{tag}", "-f", f"sha={sha}"))
                return ref
            if path.endswith("/releases/91"):
                self.assertEqual(args, ("--method", "PATCH", "-F", "draft=false",
                                        "-F", "prerelease=true", "-f", "make_latest=false"))
                return {"id": 91, "tag_name": tag, "draft": False, "prerelease": True}
            raise AssertionError(f"unexpected API: {path} {args}")

        def draft(_repository, _tag):
            self.assertEqual((_repository, _tag), (repository, tag))
            if not uploaded:
                return None
            assets = [{"name": path.name, "size": path.stat().st_size, "state": "uploaded",
                       "digest": "sha256:" + hashlib.sha256(path.read_bytes()).hexdigest()}
                      for path in uploaded]
            return {"id": 91, "tag_name": tag, "draft": True, "prerelease": True, "assets": assets}

        def command(argv, *, check):
            self.assertTrue(check)
            if argv[:3] == ["gh", "release", "create"]:
                self.assertEqual(argv[3:10], [tag, "--repo", repository, "--verify-tag",
                                              "--draft", "--prerelease", "--latest=false"])
                uploaded.extend(map(Path, argv[14:]))
                self.assertEqual(len(uploaded), 4)
            elif argv[:3] == ["gh", "run", "download"]:
                self.assertEqual(argv[3], "12")
            else:
                self.fail(f"unexpected command: {argv}")

        def manifest(directory, expected, commit, platform, run_id, attempt):
            self.assertEqual((expected, commit, run_id, attempt), (version, sha, "12", 1))
            directory.mkdir(parents=True)
            file = directory / (platform + ".test")
            file.write_bytes(b"synthetic CI artifact")
            return [file]

        with patch.dict(os.environ, {"GITHUB_REPOSITORY": repository, "GITHUB_REF": "refs/heads/main",
                                     "GITHUB_SHA": sha}), \
             patch("sys.argv", ["publish.py", "--version", version, "--run-id", "12", "--publish"]), \
             patch("publish.git", return_value=sha), \
             patch("publish.check_release", return_value=sha) as marker, \
             patch("publish.api", side_effect=api), \
             patch("publish.find_release", side_effect=draft), \
             patch("publish.subprocess.run", side_effect=command), \
             patch("publish.validate_manifest", side_effect=manifest):
            publish.main()
        marker.assert_called_once_with(version)
        self.assertEqual([params[1] for _, params in writes], ["POST", "PATCH"])
        evidence = json.loads(Path(".build/cd/release-provenance.json").read_text())
        self.assertEqual((evidence["tag"], evidence["commit"], evidence["ciAttempt"]), (tag, sha, 1))


if __name__ == "__main__":
    unittest.main()
