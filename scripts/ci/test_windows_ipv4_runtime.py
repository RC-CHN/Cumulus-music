"""Check that the passive report distinguishes cancellation from normal completion."""

import json
import unittest

from windows_ipv4_runtime import SCOPE, cancelled, categorical_report


def round_fixture(before="idle", accepted=True, terminal="cancelled"):
    return dict(
        pendingArm=True, exclusive=True, beforeClose=before, cancelAccepted=accepted,
        terminal=terminal,
    )


class Ipv4RuntimeReceipt(unittest.TestCase):
    def test_two_genuine_cancellations_and_no_false_positive_for_a_racing_change(self):
        first = round_fixture()
        result = categorical_report(json.dumps(dict(SCOPE, first=first, second=round_fixture())))
        self.assertTrue(cancelled(result["first"]) and cancelled(result["second"]))
        changed = round_fixture(terminal="changed")
        result = categorical_report(json.dumps(dict(SCOPE, first=changed, second=None)))
        self.assertFalse(cancelled(result["first"]))
        retained = round_fixture(terminal="retained")
        result = categorical_report(json.dumps(dict(SCOPE, first=retained, second=None)))
        self.assertFalse(cancelled(result["first"]))

    def test_refuses_unproved_reopen_and_private_data(self):
        rejected = [
            dict(SCOPE, first=round_fixture(accepted=False), second=round_fixture()),
            dict(SCOPE, first=round_fixture(before="changed"), second=round_fixture()),
            dict(SCOPE, first=round_fixture(), second=None),
            dict(SCOPE, first=dict(round_fixture(), address="must not escape"), second=None),
            dict(SCOPE, first=round_fixture(), second=dict(round_fixture(), ssid="must not escape")),
        ]
        for report in rejected:
            with self.subTest(first_keys=list(report["first"])):
                with self.assertRaises(ValueError):
                    categorical_report(json.dumps(report))

    def test_unarmed_and_unexpected_cancellation_are_categorical_failures(self):
        unarmed = dict(
            pendingArm=False, exclusive=None, beforeClose=None, cancelAccepted=None, terminal=None,
        )
        result = categorical_report(json.dumps(dict(SCOPE, first=unarmed, second=None)))
        self.assertFalse(cancelled(result["first"]))
        already_cancelled = round_fixture(before="cancelled", accepted=None, terminal="unarmed")
        result = categorical_report(json.dumps(dict(SCOPE, first=already_cancelled, second=None)))
        self.assertFalse(cancelled(result["first"]))


if __name__ == "__main__":
    unittest.main()
