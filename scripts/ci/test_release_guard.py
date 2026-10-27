import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from release_guard import check_history, check_release, check_versions, VERSION


class ReleaseHistory(unittest.TestCase):
    def test_one_release_per_version(self):
        self.assertEqual(check_history(["a feat: work", "b chore:release 0.1.0", "c fix: work", "d chore:release 0.1.1"]), {"0.1.0": "b", "0.1.1": "d"})

    def test_duplicate_is_rejected_even_with_intervening_fixes(self):
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            check_history(["a chore:release 0.1.0", "b fix: build", "c chore:release 0.1.0"])

    def test_two_refs_to_same_commit_are_not_two_commits(self):
        self.assertEqual(check_history(["a chore:release 0.1.0", "a chore:release 0.1.0"]), {"0.1.0": "a"})

    def test_malformed_versions_rejected(self):
        for version in ["01.1.0", "0.1", "v0.1.0", "0.1.0\n", "0.1.0; echo bad"]:
            self.assertIsNone(VERSION.fullmatch(version))
        with self.assertRaises(AssertionError):
            check_history(["a chore:release v0.1.0"])


class ReleaseCommit(unittest.TestCase):
    def setUp(self):
        self.previous = Path.cwd()
        self.directory = tempfile.TemporaryDirectory()
        root = Path(self.directory.name)
        for file in ["Cargo.toml", "Cargo.lock", "frontend/package.json", "frontend/package-lock.json", "desktop/tauri.conf.json", "docs/releases/0.1.0.txt"]:
            destination = root / file
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(self.previous / file, destination)
        os.chdir(root)
        self.git("init", "-q")
        self.git("config", "user.name", "Synthetic release test")
        self.git("config", "user.email", "test@example.invalid")
        self.git("add", ".")
        self.git("commit", "-qm", "feat: prepared source")

    def tearDown(self):
        os.chdir(self.previous)
        self.directory.cleanup()

    def git(self, *args):
        subprocess.run(["git", *args], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)

    def test_tag_target_must_be_the_unique_empty_release_marker(self):
        with self.assertRaisesRegex(AssertionError, "release commit"):
            check_release("0.1.0")
        self.git("commit", "--allow-empty", "-qm", "chore:release 0.1.0")
        self.assertEqual(len(check_release("0.1.0")), 40)
        self.git("commit", "--allow-empty", "-qm", "chore:release 0.1.0")
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            check_release("0.1.0")

    def test_unverified_content_cannot_be_hidden_in_release_marker(self):
        Path("changed").write_text("unexpected change")
        self.git("add", "changed")
        self.git("commit", "-qm", "chore:release 0.1.0")
        with self.assertRaisesRegex(AssertionError, "prepare changes"):
            check_release("0.1.0")

    def test_versions_must_agree(self):
        self.assertEqual(check_versions(), "0.1.0")
        path = Path("desktop/tauri.conf.json")
        path.write_text(path.read_text().replace('"0.1.0"', '"0.1.1"'))
        with self.assertRaisesRegex(AssertionError, "mismatch"):
            check_versions()


if __name__ == "__main__":
    unittest.main()
