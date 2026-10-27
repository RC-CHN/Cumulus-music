#!/usr/bin/env python3
"""Independently compare official responses, native queue snapshots and SQLite.
This is a read-only verifier; it does not implement or invoke music business.
"""
import argparse
import json
from pathlib import Path
import sqlite3


def read(p):
    return json.loads(p.read_text())


def records(root):
    with sqlite3.connect((root / "state/core.sqlite3").resolve().as_uri() + "?mode=ro", uri=True) as db:
        return [(o, k, key, json.loads(v)) for o, k, key, v in db.execute(
            "SELECT owner,kind,key,value FROM state WHERE kind IN ('scene-fm-v1','playback','podcast-heard-v1')")]


def ids(programs):
    return [str(v.get("id", v.get("voiceId"))) for v in programs]


def media(v):
    return str(v.get("mainTrackId") or v.get("mainSong", {}).get("id") or v.get("trackId"))


def official(rows):
    scenes = {}
    for r in rows:
        if r["path"] == "/api/voice/homepage/block/content":
            assert r["params"] == {"blockCode": "VOICE_HOMEPAGE_BLOCK_SCENE_FM"}
            for v in r["response"]["data"][0]["creatives"]:
                if v.get("creativeId"):
                    scenes[v["creativeId"]] = (v.get("creativeExtInfoVO") or {}).get("djPrograms") or []
        elif r["path"] == "/api/content/scene/fm/play/list":
            assert r["params"]["sceneCode"] == "SCENE_FM"
            assert r["params"]["sceneId"] in scenes
            scenes[r["params"]["sceneId"]] = r["response"]["data"]["list"]
    assert scenes
    return scenes


def native(root):
    report = read(root / "report.json")
    assert report["explicit_remote_writes"] == 0 and report["cache_reopen_equal"] and report["cache_no_network"]
    assert report["default_autoplay"] is False and report["actual_playback_tested"] is False
    scenes = official([read(p) for p in sorted(root.glob("read-*.json"))])
    for path in ["first-queue.json", "second-queue.json"]:
        row = read(root / path)
        programs = scenes[row["state"]["active_id"]]
        assert [r["voice_id"] for r in row["resources"]] == ids(programs)
        assert [r["id"] for r in row["resources"]] == [media(p) for p in programs]
        assert row["state"]["pending"] and row["index"] < len(programs)
    state = records(root)
    saved = [v for _, k, key, v in state if k == "scene-fm-v1" and key == "current"]
    assert saved == [read(root / "second-queue.json")["state"]]
    return {"ok": True, "proof": "native_read_and_queue_preparation", "categories": len(scenes),
            "queue_sizes": report["queue_sizes"], "remote_writes": 0, "playback_proven": False}


def run(root, folder, count):
    path = root / folder
    assert read(path / "complete.json")["ok"] and read(path / "capture.json")["ok"]
    assert not (path / "blocked-endpoint.json").exists()
    stages = {p.stem: read(p) for p in path.glob("scene_*.json")}
    assert len(stages) == count
    for name, s in stages.items():
        assert not s["overflow"] and s["nativePlayer"]["error"] is None
        assert s["coverIconWidth"] >= 47, "zero-size SVG content regressed"
        assert (path / (name + ".png")).is_file()
        assert not s["nativeScene"]["value"]["pending"]
    return stages, read(path / "api-calls.json")


def ui(root, reopen):
    stages, calls = run(root, "first", 8)
    raw = [read(p) for p in sorted((root / "first").glob("official-*.json"))]
    scenes = official(raw)
    original = stages["scene_home"]["nativePlayer"]
    assert original["state"] == "playing" and not stages["scene_home"]["nativeScene"]["value"]["active"]
    assert stages["scene_auto_guard"]["nativePlayer"]["entryId"] == original["entryId"]
    for name in ["scene_playing", "scene_paused", "scene_next", "scene_changed", "scene_eof", "scene_saved"]:
        s = stages[name]
        p, v, q = s["nativePlayer"], s["nativeScene"]["value"], s["nativeQueue"]
        assert v["active"] and not p["fm"]
        assert v["episode"]["id"] == p["track"]["voiceId"]
        assert s["episodeTitle"] == p["track"]["name"] and s["selectedScene"] == v["activeId"]
        programs = scenes[v["activeId"]]
        assert [t["track"]["voiceId"] for t in q["tracks"]] == ids(programs)
        assert [t["track"]["id"] for t in q["tracks"]] == [media(p) for p in programs]
        assert p["queueLen"] == len(programs)
        assert s["coverLabel"] == ("暂停随心听FM" if v["playing"] else "播放随心听FM")
    assert stages["scene_playing"]["nativePlayer"]["entryId"] != stages["scene_next"]["nativePlayer"]["entryId"]
    assert stages["scene_changed"]["nativePlayer"]["entryId"] != stages["scene_eof"]["nativePlayer"]["entryId"]
    assert stages["scene_playing"]["nativeScene"]["value"]["activeId"] != stages["scene_changed"]["nativeScene"]["value"]["activeId"]
    saved = stages["scene_saved"]["nativePlayer"]
    assert saved["state"] == "paused" and abs(saved["position"] - 16) < .7
    permissions = [r["params"]["voiceId"] for r in calls if r["path"] == "/api/dj/play/url/check"]
    assert saved["track"]["voiceId"] in permissions
    assert not stages["scene_saved"]["nativeScene"]["value"]["autoplay"]
    state = records(root)
    queue = next(v for _, k, key, v in state if k == "playback" and key == "queue")
    assert all(e["source"]["kind"] == "scene_fm" for e in queue["items"])
    assert queue["current"] == saved["entryId"]
    assert len({o for o, k, _, _ in state if k == "scene-fm-v1"}) == 1
    assert any(k == "podcast-heard-v1" and key == saved["track"]["voiceId"] for _, k, key, _ in state)
    assert not next(v for _, k, key, v in state if k == "scene-fm-v1" and key == "current")["pending"]
    count = 8
    if reopen:
        again, later_calls = run(root, "reopen", 2)
        p = again["scene_reopened"]["nativePlayer"]
        assert p["entryId"] == saved["entryId"] and p["state"] == "paused" and abs(p["position"] - 16) < .7
        assert again["scene_reopened"]["nativeQueue"]["tracks"] == stages["scene_saved"]["nativeQueue"]["tracks"]
        assert again["scene_resumed"]["nativePlayer"]["position"] > 16.2
        assert not any(r["path"] in ("/api/voice/homepage/block/content", "/api/content/scene/fm/play/list") for r in later_calls)
        count += 2
    return {"ok": True, "proof": "desktop", "screenshots": count, "categories": len(scenes),
            "full_queues_match_official": True, "autoplay_guard": True, "eof": True,
            "reopen": reopen, "remote_writes": 0, "audio_output": "libmpv/null"}


def links(root):
    folder = root / "first"
    complete = read(folder / "complete.json")
    assert complete["ok"] and complete["links"] and not complete["downloadStarted"]
    assert read(folder / "capture.json")["ok"]
    assert not (folder / "blocked-endpoint.json").exists()
    rows = {p.stem: read(p) for p in folder.glob("scene_link_*.json")}
    assert len(rows) == 3
    for name, row in rows.items():
        assert not row["overflow"] and (folder / (name + ".png")).is_file()
        assert row["episode"] == row["nativeScene"]["value"]["episode"]["name"]
        assert row["nativeQueue"]["tracks"] == []
    download, radio, comments = (rows["scene_link_" + s] for s in ["download", "radio", "comments"])
    assert download["downloadTitle"] == "下载 1 个单集" and download["downloadSelection"] == download["episode"]
    assert radio["radioTitle"] == radio["nativeScene"]["value"]["episode"]["radioName"] and radio["visiblePrograms"] > 0
    assert comments["commentTitle"] == comments["episode"] and int(comments["commentTotal"]) >= 0
    calls = read(folder / "api-calls.json")
    expected = "/api/v1/resource/comments/A_DJ_1_" + comments["nativeScene"]["value"]["episode"]["id"]
    assert any(c["path"] == expected for c in calls)
    assert any(c["path"] == "/api/djradio/v3/get" for c in calls)
    assert not any("download/url" in c["path"] for c in calls)
    with sqlite3.connect((root / "state/core.sqlite3").resolve().as_uri() + "?mode=ro", uri=True) as db:
        assert db.execute("SELECT COUNT(*) FROM state WHERE kind='download_tasks_v1'").fetchone()[0] == 0
    return {"ok": True, "proof": "desktop_links", "screenshots": 3, "radio_detail": True,
            "voice_comments": True, "voice_download_dialog": True, "download_started": False, "remote_writes": 0}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("root", type=Path)
    p.add_argument("--native", action="store_true")
    p.add_argument("--reopen", action="store_true")
    p.add_argument("--links", action="store_true")
    args = p.parse_args()
    result = native(args.root) if args.native else links(args.root) if args.links else ui(args.root, args.reopen)
    with (args.root / ("independent-reopen-audit.json" if args.reopen else "independent-audit.json")).open("x") as f:
        json.dump(result, f, indent=2)
        f.write("\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
