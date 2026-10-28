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


class ResumeFlow(unittest.TestCase):
    def test_recovery_only_publishes_existing_draft_and_preserves_target_ci(self):
        previous = Path.cwd()
        with tempfile.TemporaryDirectory() as tmp:
            os.chdir(tmp)
            try:
                Path("docs/releases").mkdir(parents=True)
                Path("docs/releases/0.1.0.txt").write_text("Synthetic notes")
                draft = {"id": 91, "tag_name": "v0.1.0", "draft": True, "prerelease": False, "assets": []}
                run = {"id": 12, "repository": {"full_name": "owner/repo"},
                       "head_repository": {"full_name": "owner/repo"}, "workflow_id": 42,
                       "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main",
                       "head_sha": "release-sha", "status": "completed", "conclusion": "success",
                       "run_attempt": 1, "html_url": "https://example.invalid/ci/12"}
                calls = []

                def api(path, *args):
                    calls.append((path, args))
                    if path.endswith("/git/ref/tags/v0.1.0"):
                        return {"ref": "refs/tags/v0.1.0", "object": {"type": "commit", "sha": "release-sha"}}
                    if path.endswith("/releases?per_page=100"):
                        return [[copy.deepcopy(draft)]]
                    if path.endswith("/releases/91"):
                        result = copy.deepcopy(draft)
                        if args:
                            self.assertEqual(args, ("--method", "PATCH", "-F", "draft=false", "-f", "make_latest=true"))
                            result["draft"] = False
                        return result
                    if path.endswith("/actions/workflows/ci.yml"):
                        return {"id": 42}
                    if path.endswith("/actions/runs/12"):
                        return run
                    if path.endswith("/attempts/1/jobs?per_page=100"):
                        return {"jobs": [{"name": name, "status": "completed", "conclusion": "success"} for name in publish.REQUIRED_JOBS]}
                    raise AssertionError(f"unexpected API: {path} {args}")

                def manifest(directory, version, sha, platform, run_id, attempt):
                    self.assertEqual((version, sha, run_id, attempt), ("0.1.0", "release-sha", "12", 1))
                    directory.mkdir(parents=True)
                    file = directory / (platform + ".test")
                    file.write_bytes(b"synthetic CI artifact")
                    return [file]

                with patch.dict(os.environ, {"GITHUB_REPOSITORY": "owner/repo", "GITHUB_REF": "refs/heads/main", "GITHUB_SHA": "recovery-sha"}), \
                     patch("sys.argv", ["publish.py", "--version", "0.1.0", "--run-id", "12", "--publish", "--resume-draft"]), \
                     patch("publish.git", return_value="recovery-sha"), \
                     patch("publish.check_release", return_value="release-sha") as marker, \
                     patch("publish.api", side_effect=api), \
                     patch("publish.subprocess.run") as command, \
                     patch("publish.validate_manifest", side_effect=manifest), \
                     patch("publish.validate_uploaded") as uploaded:
                    publish.main()
                marker.assert_called_once_with("0.1.0", "release-sha")
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


if __name__ == "__main__":
    unittest.main()
