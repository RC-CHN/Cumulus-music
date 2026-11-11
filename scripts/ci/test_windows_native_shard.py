import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from windows_native_shard import SHARDS, owner, verify


class WindowsNativeShardTest(unittest.TestCase):
    run_id = ["synthetic-sha", "123", "2"]
    suites = [("core", "deps/download_directory"), ("core", "deps/relay_publication"),
              ("core", "deps/client_downloads"), ("desktop", "examples/catalog_ui_probe"),
              ("core", "deps/client_player")]

    def verify_fixture(self, edit=lambda rows: None):
        with TemporaryDirectory() as directory, patch.dict("os.environ", {
            "GITHUB_SHA": self.run_id[0], "GITHUB_RUN_ID": self.run_id[1],
            "GITHUB_RUN_ATTEMPT": self.run_id[2],
        }):
            paths = [Path(directory, f"{shard}.jsonl") for shard in SHARDS]
            rows = [[[target, shard, 0 if owner(target) == shard else None, self.run_id.copy()]
                     for target in self.suites] for shard in SHARDS]
            edit(rows)
            for path, entries in zip(paths, rows, strict=True):
                path.write_text("".join(json.dumps(row) + "\n" for row in entries), encoding="utf-8")
            verify(paths)

    def test_complete_and_current_inventories_pass(self):
        self.verify_fixture()

    def test_missing_target_from_one_cargo_invocation_fails(self):
        for shard in (1, 2):
            with self.subTest(shard=SHARDS[shard]), self.assertRaisesRegex(ValueError, "inventories differ"):
                self.verify_fixture(lambda rows: rows[shard].pop(3))

    def test_missing_third_inventory_fails(self):
        with self.assertRaisesRegex(ValueError, "exactly one Windows Rust inventory per shard"):
            verify(["a.jsonl", "b.jsonl"])

    def test_failed_duplicate_and_prior_attempt_receipts_fail(self):
        for label, edit in (
            ("failed owner", lambda rows: rows[0][0].__setitem__(2, 1)),
            ("second owner", lambda rows: rows[1][0].__setitem__(2, 0)),
            ("missing third owner", lambda rows: rows[2][2].__setitem__(2, None)),
            ("duplicate target", lambda rows: rows[0].append(rows[0][0])),
            ("prior attempt", lambda rows: rows[0][0][3].__setitem__(2, "1")),
        ):
            with self.subTest(label), self.assertRaisesRegex(ValueError, "invalid or unsuccessful"):
                self.verify_fixture(edit)


if __name__ == "__main__":
    unittest.main()
