#!/usr/bin/env python3
"""Read-only inspection of FM mode checkpoints, requests and durable state."""
import argparse
import json
from pathlib import Path
import sqlite3


def read(path):
    return json.loads(path.read_text())


def records(root):
    with sqlite3.connect((root / "state/core.sqlite3").resolve().as_uri() + "?mode=ro", uri=True) as db:
        return [(o, k, key, json.loads(v)) for o, k, key, v in db.execute(
            "SELECT owner,kind,key,value FROM state WHERE kind LIKE 'fm-%' OR kind='playback'")]


def native(root):
    report = read(root / "report.json")
    assert report["explicit_remote_writes"] == 0 and report["cache_reopen_equal"]
    raw = [read(p) for p in sorted(root.glob("read-*.json"))]
    familiar = [r for r in raw if r["path"] == "/api/v1/radio/get" and r["params"].get("mode") == "FAMILIAR"]
    assert familiar and all(len(json.loads(r["params"]["sourceIds"])) > 1 for r in familiar)
    refs = [r for r in raw if r["path"] == "/api/tag/tab/fm/recommends"]
    details = [r for r in raw if r["path"] == "/api/v3/song/detail"]
    assert refs and len(refs) == len(details)
    for r, d in zip(refs, details):
        assert r["params"]["ruleType"] == 1 and r["params"]["limit"] == 3
        assert d["params"]["trialMode"] == -1
        assert [str(x["id"]) for x in json.loads(d["params"]["c"])] == [str(x["id"]) for x in r["response"]["data"]]
    state = records(root)
    queues = [v for _, k, _, v in state if k == "fm-queues-v1"]
    assert {q["source"]["kind"] for q in queues} == {"familiar", "zone"}
    for p in root.glob("queue-*.json"):
        q = read(p)
        assert q in queues and q["tracks"] and q["index"] < len(q["tracks"])
    assert len({o for o, k, _, _ in state if k == "fm-queues-v1"}) == 1
    assert not any("mutations" in k or k == "fm-skip-v1" for _, k, _, _ in state)
    return {"ok": True, "proof": "native_read", "modes": ["familiar", "zone"], "queues": len(queues), "remote_mutations": 0}


def run(root, name, count):
    folder = root / name
    assert read(folder / "complete.json")["ok"]
    assert read(folder / "capture.json")["ok"]
    stages = {p.stem: read(p) for p in folder.glob("mode_*.json")}
    assert len(stages) == count
    for key, row in stages.items():
        assert not row["overflow"] and row["nativePlayer"]["error"] is None
        assert (folder / (key + ".png")).is_file()
    calls = read(folder / "api-calls.json")
    assert not (folder / "blocked-endpoint.json").exists()
    assert not any(c["path"] in ("/api/v1/radio/skip", "/api/radio/trash/add", "/api/radio/trash/del/batch", "/api/music-blacklist/add", "/api/music-blacklist/delete") for c in calls)
    return stages, calls


def ui(root, reopen):
    s, calls = run(root, "first", 8)
    p = {k: v["nativePlayer"] for k, v in s.items()}
    assert p["mode_original"]["state"] == "paused" and not p["mode_original"]["fm"]
    assert s["mode_cards"]["zoneCount"] > 0
    compact = "totalZoneCount" in s["mode_cards"]
    if compact:
        assert s["mode_cards"]["zoneCount"] == 5 and s["mode_cards"]["totalZoneCount"] > 5
    for key, kind, trial in [("mode_familiar", "familiar", 36), ("mode_zone", "zone", 121)]:
        assert p[key]["fm"] and p[key]["state"] == "playing" and p[key]["position"] > .2
        assert s[key]["nativeFm"]["value"]["activeSource"]["kind"] == kind
        assert any(c["path"] == "/api/song/enhance/player/url/v1" and c["params"]["trialMode"] == trial for c in calls)
    assert p["mode_zone_advanced"]["entryId"] != p["mode_zone"]["entryId"]
    assert p["mode_zone_advanced"]["fm"] and p["mode_zone_advanced"]["state"] == "playing"
    assert s["mode_returned"]["nativeQueue"]["tracks"] == s["mode_original"]["nativeQueue"]["tracks"]
    assert not p["mode_returned"]["fm"] and abs(p["mode_returned"]["position"] - 12) < .7
    assert p["mode_saved"]["state"] == "paused" and abs(p["mode_saved"]["position"] - 14) < .7
    count = 8
    if reopen:
        r, _ = run(root, "reopen", 3)
        restored = r["mode_reopened"]["nativePlayer"]
        assert r["mode_reopened"]["source"]["kind"] == "zone"
        assert restored["entryId"] == p["mode_saved"]["entryId"] and abs(restored["position"] - 14) < .7
        assert r["mode_reopened_playing"]["nativePlayer"]["state"] == "playing"
        back = r["mode_reopened_returned"]["nativePlayer"]
        assert not back["fm"] and back["entryId"] == p["mode_original"]["entryId"] and abs(back["position"] - 12) < .7
        assert r["mode_reopened_returned"]["nativeQueue"]["tracks"] == s["mode_original"]["nativeQueue"]["tracks"]
        count += 3
    state = records(root)
    assert {v["source"]["kind"] for _, k, _, v in state if k == "fm-queues-v1"} == {"personal", "familiar", "zone"}
    assert len({o for o, k, _, _ in state if k == "fm-queues-v1"}) == 1
    assert not any("mutations" in k or k == "fm-skip-v1" for _, k, _, _ in state)
    backup = [v for _, k, key, v in state if k == "playback" and key == "fm-backup-queue"]
    assert bool(backup) != reopen
    if backup:
        assert backup[0]["order"] == [t["entryId"] for t in s["mode_original"]["nativeQueue"]["tracks"]]
    return {"ok": True, "proof": "desktop", "screenshots": count, "modes": ["familiar", "zone"], "zone_trial_mode": 121, "zone_eof": True, "normal_queue_restored": True, "reopen": reopen, "compact_navigation":compact, "remote_mutations": 0, "audio_output": "libmpv/null"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--reopen", action="store_true")
    args = parser.parse_args()
    result = native(args.root) if args.native else ui(args.root, args.reopen)
    target = args.root / ("independent-reopen-audit.json" if args.reopen else "independent-audit.json")
    with target.open("x") as f:
        json.dump(result, f, indent=2)
        f.write("\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
