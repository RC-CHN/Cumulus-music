#!/usr/bin/env python3
"""Independently inspect FM UI/native checkpoints and persisted queue ownership.

Python performs no business operations and never replays feedback requests.
"""
import argparse
import json
from pathlib import Path
import sqlite3


def read(path):
    return json.loads(path.read_text())


def verify_run(root, name, expected):
    run = root / name
    assert read(run / "complete.json")["ok"]
    capture = read(run / "capture.json")
    assert capture["ok"] and capture["screenshots"] == expected
    stages = {p.stem: read(p) for p in run.glob("fm_*.json")}
    assert len(stages) == expected
    for stage, value in stages.items():
        assert not value["overflow"]
        assert (run / (stage + ".png")).is_file()
        assert value["nativePlayer"]["error"] is None
    assert not (run / "blocked-endpoint.json").exists()
    calls = read(run / "api-calls.json")
    forbidden = {"/api/v1/radio/skip", "/api/radio/trash/add", "/api/radio/trash/del/batch"}
    assert not any(call["path"] in forbidden for call in calls)
    assert any(c["path"] == "/api/song/enhance/player/url/v1"
               and c["params"]["trialMode"] == 36 for c in calls)
    return stages


def player(stages, key):
    return stages[key]["nativePlayer"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--reopen", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    stages = verify_run(root, "first", 9)
    original = player(stages, "fm_original")
    playing = player(stages, "fm_playing")
    repeat = player(stages, "fm_repeat")
    replayed = player(stages, "fm_repeat_replayed")
    advanced = player(stages, "fm_advanced")
    saved = player(stages, "fm_saved")
    assert not original["fm"] and original["queueLen"] > 1
    assert original["state"] == "paused" and abs(original["position"] - 12) < .7
    assert playing["fm"] and playing["state"] == "playing" and playing["position"] > .2
    assert repeat["entryId"] == playing["entryId"] and repeat["mode"] == "single_loop"
    assert repeat["playId"] == playing["playId"] and abs(repeat["position"] - 12) < .7
    assert replayed["entryId"] == playing["entryId"] and replayed["playId"] != playing["playId"]
    assert replayed["state"] == "playing" and 0 < replayed["position"] < 8
    assert advanced["entryId"] != playing["entryId"] and advanced["state"] == "playing"
    assert advanced["fm"] and advanced["mode"] == "sequential"
    assert stages["fm_trash"]["selected"] == 1 and stages["fm_trash"]["loaded"] > 0
    assert stages["fm_returned"]["nativeQueue"] == stages["fm_original"]["nativeQueue"] or (
        # Queue revision advances on restoration; the complete ordered entries do not.
        stages["fm_returned"]["nativeQueue"]["tracks"] == stages["fm_original"]["nativeQueue"]["tracks"]
        and stages["fm_returned"]["nativeQueue"]["current"] == original["entryId"])
    returned = player(stages, "fm_returned")
    assert not returned["fm"] and returned["state"] == "paused" and abs(returned["position"] - 12) < .7
    assert saved["fm"] and saved["state"] == "paused" and abs(saved["position"] - 14) < .7
    count = 9
    if args.reopen:
        second = verify_run(root, "reopen", 3)
        restored = player(second, "fm_reopened")
        assert restored["fm"] and restored["state"] == "paused"
        assert restored["entryId"] == saved["entryId"] and abs(restored["position"] - 14) < .7
        resumed = player(second, "fm_reopened_playing")
        assert resumed["state"] == "playing" and resumed["position"] > 14.2
        normal = player(second, "fm_reopened_returned")
        assert not normal["fm"] and normal["entryId"] == original["entryId"]
        assert normal["state"] == "paused" and abs(normal["position"] - 12) < .7
        assert second["fm_reopened_returned"]["nativeQueue"]["tracks"] == stages["fm_original"]["nativeQueue"]["tracks"]
        count += 3
    with sqlite3.connect((root / "state/core.sqlite3").as_uri() + "?mode=ro", uri=True) as db:
        records = {(owner, kind, key): json.loads(value) for owner, kind, key, value in db.execute(
            "SELECT owner,kind,key,value FROM state WHERE kind LIKE 'fm-%' OR kind='playback'")}
    assert len({owner for owner, kind, _ in records if kind == "fm-queues-v1"}) == 1
    assert not any(kind in ("fm-skip-v1", "fm-trash-mutations-v1") for _, kind, _ in records)
    backup = [v for (_, kind, key), v in records.items() if kind == "playback" and key == "fm-backup-queue"]
    assert bool(backup) != args.reopen
    if backup:
        assert backup[0]["order"] == [row["entryId"] for row in stages["fm_original"]["nativeQueue"]["tracks"]]
    result = {"ok": True, "screenshots": count, "real_audio_engine": "libmpv/null",
              "single_repeat": True, "natural_advance": True, "normal_queue_restored": True,
              "reopen_verified": args.reopen, "remote_mutations": 0, "fm_trial_mode": 36}
    path = root / ("independent-reopen-audit.json" if args.reopen else "independent-audit.json")
    with path.open("x") as output:
        json.dump(result, output, indent=2)
        output.write("\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
