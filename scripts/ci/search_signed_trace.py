"""Strict accounting for the sealed, genuine PC assignment transport.

The full native assignment is recorded by MusicHttpClient's backend, separately
from the legacy search fixture's result counters. Count both without allowing a
duplicate assignment, a short assignment, or an extra result read to pass.
"""
import json


ACCOUNT = "/api/w/nuser/account/get"
ASSIGNMENT = "/api/rtrs/abt/front/expinfo/list"


def assignment_arguments(repository):
    manifest = json.loads((repository / "core/src/protocol/pc_experiments_3_1_42.json")
                          .read_text(encoding="utf-8"))
    names = [name for name, opened, _ in manifest["web"] if opened]
    return {"expNames": ",".join(names + manifest["native"])}


def signed_assignment(trace, case, expected):
    assert trace["transport"] == "MusicHttpClient" and trace["syntheticOnly"] is True
    assert trace["sameProfileAccountRestore"] is True and trace["footerCase"] == case
    calls = trace["calls"]
    assert [call["path"] for call in calls] == [ACCOUNT, ASSIGNMENT], calls
    assert calls[0]["args"] == {} and calls[1]["args"] == expected, calls
    assert all(call["signedPcContextVerified"] is True and call["sameAccountCredential"] is True
               for call in calls), "real EAPI/context/same-profile backend assertions must pass"
    assert calls[0]["responseCode"] == 200
    if case == "error":
        assert calls[1].get("response") == "synthetic_error" and "responseCode" not in calls[1]
    else:
        assert calls[1]["responseCode"] == 200


def comprehensive_reads(complete, expected_assignment, result_reads, expected_total):
    """One signed full assignment plus the exact number of fixture result reads."""
    signed_assignment(complete["signedSearch"], "t1", expected_assignment)
    assert type(result_reads) is int and result_reads >= 0, result_reads
    assert result_reads + 1 == expected_total, {
        "signedFullAssignmentReads": 1, "resultReads": result_reads,
        "expectedTotal": expected_total,
    }
    return expected_total
