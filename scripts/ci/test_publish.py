import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from publish import validate_manifest, validate_run


class PublishGate(unittest.TestCase):
    def setUp(self):
        self.run = {"id": 12, "repository": {"full_name": "owner/repo"}, "head_repository": {"full_name": "owner/repo"}, "workflow_id": 42, "path": ".github/workflows/ci.yml", "event": "push", "head_branch": "main", "head_sha": "abc", "status": "completed", "conclusion": "success"}

    def test_exact_success(self):
        validate_run(self.run, "owner/repo", "abc", "12", 42)

    def test_previous_commit_partial_ci_fork_or_wrong_workflow_rejected(self):
        for key, value in [("head_sha", "previous"), ("status", "in_progress"), ("conclusion", "failure"), ("conclusion", "cancelled"), ("event", "pull_request"), ("head_branch", "other"), ("workflow_id", 99), ("path", ".github/workflows/other.yml"), ("head_repository", {"full_name": "attacker/repo"})]:
            with self.subTest(key=key, value=value):
                run = copy.deepcopy(self.run)
                run[key] = value
                with self.assertRaises(AssertionError):
                    validate_run(run, "owner/repo", "abc", "12", 42)

    def test_digest_attempt_and_extra_asset_checked(self):
        with tempfile.TemporaryDirectory() as tmp:
            directory = Path(tmp)
            names = ["Cumulus-0.1.0-linux-x64.deb", "Cumulus-0.1.0-linux-x64.tar.gz"]
            for name in names:
                (directory / name).write_bytes(b"synthetic package")
            manifest = {"version": "0.1.0", "commit": "abc", "platform": "linux-x64", "ciRun": "12", "ciAttempt": "2", "nativePlayback": {"ok": True, "remote_requests": 0}, "assets": [{"name": name, "bytes": 17, "sha256": hashlib.sha256(b"synthetic package").hexdigest()} for name in names]}
            (directory / "manifest-linux-x64.json").write_text(json.dumps(manifest))
            self.assertEqual(len(validate_manifest(directory, "0.1.0", "abc", "linux-x64", 12, 2)), 3)
            with self.assertRaisesRegex(AssertionError, "attempt"):
                validate_manifest(directory, "0.1.0", "abc", "linux-x64", 12, 3)
            (directory / names[0]).write_bytes(b"modified package!")
            with self.assertRaisesRegex(AssertionError, "digest"):
                validate_manifest(directory, "0.1.0", "abc", "linux-x64", 12, 2)
            (directory / "unexpected").touch()
            with self.assertRaisesRegex(AssertionError, "extra"):
                validate_manifest(directory, "0.1.0", "abc", "linux-x64", 12, 2)


if __name__ == "__main__":
    unittest.main()
