"""Check that the runtime gate preserves only known NLA outcome categories."""

import json
import unittest

from windows_nla_runtime import REPORT_FACTS, categorical_report, verified_runtime


def fixture(sample, online, next_call, end="ok"):
    return dict(
        REPORT_FACTS,
        sample=sample,
        officialOnline=online,
        calls=dict(
            startup="ok", begin="ok", next=next_call, getLastError=next_call == "error",
            end=end, cleanup="ok",
        ),
    )


class NativeNlaReport(unittest.TestCase):
    def test_provider_absence_is_a_valid_local_read_but_cleanup_failure_is_not(self):
        absent = fixture("NoMoreProviders", False, "error")
        self.assertTrue(verified_runtime(categorical_report(json.dumps(absent))))
        denied_end = fixture("ProviderPresent", True, "ok", end="error")
        outcome = categorical_report(json.dumps(denied_end))
        self.assertTrue(outcome["officialOnline"])
        self.assertFalse(verified_runtime(outcome))

    def test_unknown_fields_and_inconsistent_classification_are_never_preserved(self):
        valid = fixture("ProviderResultTooSmall", True, "error")
        changed = [
            dict(valid, providerName="must not escape"),
            dict(valid, calls=dict(valid["calls"], providerAddress="must not escape")),
            dict(valid, officialOnline=False),
            dict(valid, calls=dict(valid["calls"], getLastError=False)),
        ]
        for report in changed:
            with self.subTest(keys=list(report)):
                with self.assertRaises(ValueError):
                    categorical_report(json.dumps(report))


if __name__ == "__main__":
    unittest.main()
