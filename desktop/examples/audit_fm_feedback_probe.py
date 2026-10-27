#!/usr/bin/env python3
"""Read-only verification of FM UI feedback and exact account-list restoration."""
import argparse
import json
from pathlib import Path
import sqlite3


def read(path):
    return json.loads(path.read_text())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--lost-reply", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    run = root / "first"
    plan = read(root / "plan.json")
    done = read(run / "complete.json")
    assert done["ok"] and done["cleanupOk"]
    capture = read(run / "capture.json")
    assert capture["ok"] and capture["screenshots"] == 5
    assert not (run / "blocked-endpoint.json").exists()
    stages = {p.stem: read(p) for p in run.glob("feedback_*.json")}
    assert len(stages) == 5
    for name, value in stages.items():
        assert not value["overflow"] and (run / (name + ".png")).is_file()
        assert value["nativePlayer"]["error"] is None
    song = plan["song_id"]
    artist = plan["artist_id"]
    assert song not in plan["trash"] and artist not in plan["blacklist"]["artists"]
    before = stages["feedback_ready"]
    changed = stages["feedback_changed"]
    settled = stages["feedback_reconciled"]
    assert before["nativePlayer"]["entryId"] == plan["entry_id"]
    assert changed["nativePlayer"]["entryId"] != plan["entry_id"]
    assert changed["nativePlayer"]["state"] in ("playing", "buffering")
    assert song in changed["observed"]["trash"]
    assert artist in changed["observed"]["blacklist"]["artists"]
    if args.lost_reply:
        assert any(op["kind"] == "artist" and op["phase"] == "uncertain"
                   for op in changed["nativeFm"]["value"]["operations"])
    assert not settled["nativeFm"]["value"]["operations"]
    removed = stages["feedback_song_restored"]["observed"]
    assert song not in removed["trash"] and artist in removed["blacklist"]["artists"]
    restored = stages["feedback_restored"]["observed"]
    assert set(restored["trash"]) == set(plan["trash"])
    assert restored["blacklist"] == plan["blacklist"]
    baseline = read(run / "baseline-verified.json")
    assert baseline["ok"] and baseline["trash_restored"] and baseline["blacklist_restored"]
    calls = read(run / "api-calls.json")
    writes = [c for c in calls if c["mutation"]]
    expected = ["/api/radio/trash/add", "/api/music-blacklist/add",
                "/api/radio/trash/del/batch", "/api/music-blacklist/delete"]
    assert [c["path"] for c in writes] == expected
    assert all(c["response"] == 200 for c in writes)
    assert writes[0]["params"]["songId"] == song and writes[0]["params"]["algorithm_present"]
    assert writes[1]["params"] == {"contentType": "artist", "contentIdList": [artist]}
    assert writes[2]["params"] == {"songIds": [song]}
    assert writes[3]["params"] == writes[1]["params"]
    assert sum(bool(c["simulated_lost_reply"]) for c in writes) == int(args.lost_reply)
    assert not any(c["path"] == "/api/v1/radio/skip" for c in calls)
    with sqlite3.connect((root / "state/core.sqlite3").as_uri() + "?mode=ro", uri=True) as db:
        rows = [(owner, kind, json.loads(value)) for owner, kind, value in db.execute(
            "SELECT owner,kind,value FROM state WHERE kind IN ('fm-trash-mutations-v1','fm-artist-mutations-v1')")]
    assert len(rows) == 4 and {r[0] for r in rows} == {plan["owner"]}
    assert all(r[2]["phase"] == "confirmed" for r in rows)
    song_add = next(v for _, kind, v in rows if kind == "fm-trash-mutations-v1" and v["desired"])
    artist_add = next(v for _, kind, v in rows if kind == "fm-artist-mutations-v1" and v["desired"])
    assert song_add["id"] == artist_add["id"] and song_add["artist_id"] == artist
    result = {"ok": True, "screenshots": 5, "explicit_remote_writes": 4,
              "only_new_test_membership_changed": True, "full_baseline_restored": True,
              "simulated_lost_artist_reply": args.lost_reply, "no_write_replay": True}
    with (root / "independent-audit.json").open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
