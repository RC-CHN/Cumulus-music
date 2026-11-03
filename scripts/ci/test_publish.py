import copy
import hashlib
import json
from pathlib import Path
import tempfile
import unittest

from native_receipts import PINNED_CHECKS
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
            manifest["equalizerPlayback"] = {"ok": True, "remote_requests": 0}
            manifest["pinnedPlayback"] = dict.fromkeys(PINNED_CHECKS, True) | {"remote_requests": 0}
            stats = {"active_instances": 2, "instances_created": 2, "processed_samples": 4096, "failed_blocks": 0,
                     "revision": 1, "observed_revision": 1}
            bypass = stats | {"revision": 2, "observed_revision": 2, "processed_samples": 8192}
            manifest["parametricPlayback"] = {"nativeLibmpv": True, "hardwareOutput": False, "cases": [
                {"sourceSampleRate": rate, "stats": stats.copy(), "bypassStats": bypass.copy()} for rate in (44100, 96000)
            ]}
            manifest_path = directory / "manifest-linux-x64.json"
            manifest_path.write_text(json.dumps(manifest))
            self.assertEqual(len(validate_manifest(directory, "0.1.0", "abc", "linux-x64", 12, 2)), 3)
            for field, replacement, message in [
                ("equalizerPlayback", None, "equalizer"),
                ("pinnedPlayback", {"ok": True, "remote_requests": 0}, "pinned"),
                ("pinnedPlayback", manifest["pinnedPlayback"] | {"ncm_aac_m4a": False}, "pinned"),
                ("pinnedPlayback", manifest["pinnedPlayback"] | {"remote_requests": 1}, "pinned"),
                ("parametricPlayback", None, "parametric"),
                ("parametricPlayback", manifest["parametricPlayback"] | {"cases": [
                    manifest["parametricPlayback"]["cases"][0],
                    {"sourceSampleRate": 96000, "stats": stats, "bypassStats": bypass | {"instances_created": 4}},
                ]}, "parametric"),
            ]:
                with self.subTest(field=field, replacement=replacement):
                    manifest_path.write_text(json.dumps(manifest | {field: replacement}))
                    with self.assertRaisesRegex(AssertionError, message):
                        validate_manifest(directory, "0.1.0", "abc", "linux-x64", 12, 2)
            manifest_path.write_text(json.dumps(manifest))
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
