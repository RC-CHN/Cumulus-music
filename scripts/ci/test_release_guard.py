import os
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from release_guard import check_history, check_release, check_versions, is_prerelease, VERSION


class ReleaseHistory(unittest.TestCase):
    def test_one_release_per_version(self):
        self.assertEqual(check_history(["a feat: work", "b chore:release 0.1.0", "c fix: work", "d chore:release 0.1.1"]), {"0.1.0": "b", "0.1.1": "d"})

    def test_duplicate_is_rejected_even_with_intervening_fixes(self):
        with self.assertRaisesRegex(AssertionError, "duplicate"):
            check_history(["a chore:release 0.1.0", "b fix: build", "c chore:release 0.1.0"])

    def test_two_refs_to_same_commit_are_not_two_commits(self):
        self.assertEqual(check_history(["a chore:release 0.1.0", "a chore:release 0.1.0"]), {"0.1.0": "a"})

    def test_rcs_and_stable_are_distinct_full_versions(self):
        versions = ("0.2.0-rc.1", "0.2.0-rc.2", "0.2.0")
        history = [f"{i} chore:release {version}" for i, version in enumerate(versions)]
        self.assertEqual(check_history(history), {version: str(i) for i, version in enumerate(versions)})
        self.assertTrue(is_prerelease(versions[0]))
        self.assertFalse(is_prerelease(versions[-1]))
        with self.assertRaisesRegex(AssertionError, "duplicate release commit: 0.2.0-rc.1"):
            check_history([*history, "other chore:release 0.2.0-rc.1"])

    def test_malformed_versions_rejected(self):
        for version in [
            "01.1.0", "0.1", "v0.1.0", "0.1.0\n", "0.1.0; echo bad",
            "0.2.0-rc", "0.2.0-rc.0", "0.2.0-rc.01", "0.2.0-RC.1", "0.2.0-rc.1+build",
            "0.2.0-rc.1.2", "0.2.0-rc.1/foo", "0.2.0-beta.1", "0.2.0-rc.1٢",
        ]:
            with self.subTest(version=version):
                self.assertIsNone(VERSION.fullmatch(version))
        with self.assertRaises(AssertionError):
            check_history(["a chore:release v0.1.0"])


class ReleaseCommit(unittest.TestCase):
    def setUp(self):
        self.previous = Path.cwd()
        self.directory = tempfile.TemporaryDirectory()
        root = Path(self.directory.name)
        fixtures = {
            "Cargo.toml": '[workspace.package]\nversion = "0.1.0"\n',
            "Cargo.lock": '\n'.join(f'[[package]]\nname = "{name}"\nversion = "0.1.0"\n' for name in ['cumulus-core', 'cumulus-desktop', 'cumulus-audio-filter']),
            "frontend/package.json": json.dumps({"version": "0.1.0"}),
            "frontend/package-lock.json": json.dumps({"version": "0.1.0", "packages": {"": {"version": "0.1.0"}}}),
            "desktop/tauri.conf.json": json.dumps({"version": "0.1.0"}),
            "docs/releases/0.1.0.txt": "Synthetic release notes",
        }
        for file, content in fixtures.items():
            destination = root / file
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_text(content, encoding="utf-8")
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

    def test_rc_requires_matching_version_files_notes_and_marker(self):
        version = "0.2.0-rc.1"
        for filename in ["Cargo.toml", "Cargo.lock", "frontend/package.json",
                         "frontend/package-lock.json", "desktop/tauri.conf.json"]:
            path = Path(filename)
            path.write_text(path.read_text().replace("0.1.0", version))
        with self.assertRaisesRegex(AssertionError, "release notes missing"):
            check_versions()
        Path(f"docs/releases/{version}.txt").write_text("Synthetic RC notes")
        self.assertEqual(check_versions(), version)
        self.git("add", ".")
        self.git("commit", "-qm", "build: prepare release candidate")
        with self.assertRaisesRegex(AssertionError, "release commit"):
            check_release(version)
        self.git("commit", "--allow-empty", "-qm", f"chore:release {version}")
        self.assertEqual(len(check_release(version)), 40)
        with self.assertRaisesRegex(AssertionError, "differs from workspace"):
            check_release("0.2.0")

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
