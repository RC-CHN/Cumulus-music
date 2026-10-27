import unittest
from release_guard import check_history, VERSION


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


if __name__ == "__main__":
    unittest.main()
