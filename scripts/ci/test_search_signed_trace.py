"""Validator-only cases; these do not stand in for a real WebView run."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

from search_signed_trace import (
    ACCOUNT, ASSIGNMENT, assignment_arguments, comprehensive_reads, signed_assignment,
)


def trace_fixture():
    # A short placeholder schema is sufficient for negative validator tests.
    # Actual probes derive the complete ordered schema from the pinned manifest.
    expected = {"expNames": "opened-web,native,native"}
    calls = [{"path": ACCOUNT, "args": {}, "responseCode": 200},
             {"path": ASSIGNMENT, "args": expected, "responseCode": 200}]
    for call in calls:
        call.update(signedPcContextVerified=True, sameAccountCredential=True)
    return {"transport": "MusicHttpClient", "syntheticOnly": True,
            "sameProfileAccountRestore": True, "footerCase": "t1", "calls": calls}, expected


class SignedSearchTrace(unittest.TestCase):
    def test_full_assignment_and_exact_results_preserve_three_existing_budgets(self):
        for result_reads, total in ((2, 3), (6, 7), (1, 2)):
            with self.subTest(total=total):
                trace, expected = trace_fixture()
                self.assertEqual(comprehensive_reads({"signedSearch": trace}, expected,
                                                     result_reads, total), total)

    def test_missing_or_extra_result_read_does_not_pass_an_exact_budget(self):
        for count in (0, 1, 3, 4, True, 2.0):
            with self.subTest(count=count):
                trace, expected = trace_fixture()
                with self.assertRaises(AssertionError):
                    comprehensive_reads({"signedSearch": trace}, expected, count, 3)

    def test_missing_or_duplicate_full_assignment_is_rejected(self):
        for extra in (False, True):
            trace, expected = trace_fixture()
            trace["calls"] = (trace["calls"] + [copy.deepcopy(trace["calls"][1])]
                              if extra else trace["calls"][:1])
            with self.subTest(extra=extra), self.assertRaises(AssertionError):
                comprehensive_reads({"signedSearch": trace}, expected, 2, 3)

    def test_short_or_reordered_assignment_is_rejected(self):
        for args in ({"expNames": "opened-web"}, {"expNames": "native,opened-web,native"},
                     {"expNames": "opened-web,native,native", "extra": True}):
            trace, expected = trace_fixture()
            trace["calls"][1]["args"] = args
            with self.subTest(args=args), self.assertRaises(AssertionError):
                signed_assignment(trace, "t1", expected)

    def test_unverified_or_different_credential_call_is_rejected(self):
        for index in (0, 1):
            for field in ("signedPcContextVerified", "sameAccountCredential"):
                trace, expected = trace_fixture()
                trace["calls"][index][field] = False
                with self.subTest(index=index, field=field), self.assertRaises(AssertionError):
                    signed_assignment(trace, "t1", expected)

    def test_account_restore_order_and_cohort_remain_mandatory(self):
        for mutation in ("restore", "order", "cohort", "transport", "response"):
            trace, expected = trace_fixture()
            if mutation == "restore":
                trace["sameProfileAccountRestore"] = False
            elif mutation == "order":
                trace["calls"].reverse()
            elif mutation == "cohort":
                trace["footerCase"] = "c"
            elif mutation == "transport":
                trace["transport"] = "synthetic-group-provider"
            else:
                trace["calls"][1]["responseCode"] = 403
            with self.subTest(mutation=mutation), self.assertRaises(AssertionError):
                signed_assignment(trace, "t1", expected)

    def test_existing_footer_error_case_requires_an_actual_failed_response(self):
        trace, expected = trace_fixture()
        trace["footerCase"] = "error"
        with self.assertRaises(AssertionError):
            signed_assignment(trace, "error", expected)
        del trace["calls"][1]["responseCode"]
        trace["calls"][1]["response"] = "synthetic_error"
        signed_assignment(trace, "error", expected)

    def test_manifest_filters_only_closed_web_entries_and_keeps_order_and_duplicates(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = root / "core/src/protocol/pc_experiments_3_1_42.json"
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps({"web": [["closed", False, False],
                ["opened-web", True, False]], "native": ["native", "native"]}), encoding="utf-8")
            self.assertEqual(assignment_arguments(root), trace_fixture()[1])


if __name__ == "__main__":
    unittest.main()
