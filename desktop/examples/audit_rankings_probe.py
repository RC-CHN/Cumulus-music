#!/usr/bin/env python3
"""Read-only independent checks of ranking cache, UI stages and native playback."""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    root = parser.parse_args().root.resolve()
    run = root / "first"
    complete = json.loads((run / "complete.json").read_text())
    capture = json.loads((run / "capture.json").read_text())
    assert complete["ok"] and capture["ok"]
    expected = complete["screenshots"]
    assert capture["screenshots"] == expected
    stages = list(run.glob("rank_*.json"))
    assert len(stages) == expected
    for path in stages:
        assert not json.loads(path.read_text())["overflow"]
        assert path.with_suffix(".png").is_file()
    db_path = root / "state/core.sqlite3"
    with sqlite3.connect(db_path.as_uri() + "?mode=ro", uri=True) as db:
        caches = [(owner, key, json.loads(value)) for owner, key, value in db.execute(
            "SELECT owner,key,value FROM state WHERE kind='ranking-pages-v1'")]
    assert len({owner for owner, _, _ in caches}) == 1
    by_query = {}
    for _, key, saved in caches:
        encoded = json.dumps(saved["query"], ensure_ascii=False, separators=(",", ":")).encode()
        assert hashlib.sha256(encoded).hexdigest() == key
        assert saved["fetched_at"] > 0
        by_query[saved["query"]["kind"]] = saved
    assert set(by_query) == {"home", "album_sales", "album_year", "song_sales", "album"}
    home = by_query["home"]["page"]
    cards = [card for group in home["groups"] for card in group["cards"]]
    stage = json.loads((run / "rank_home.json").read_text())
    assert stage["cards"] == len(cards) and stage["groups"] == len(home["groups"])
    specials = {card["id"]: card for card in cards if card["id"] in ["albumRank", "songIndexRank"]}
    assert set(specials) == {"albumRank", "songIndexRank"}
    assert specials["albumRank"]["target"] == "album_sales"
    assert specials["songIndexRank"]["target"] == "song_sales"
    assert all(not card["canPlay"] for card in specials.values())
    assert len(by_query["album_sales"]["page"]["boards"]) == 8
    history = by_query["album_year"]["page"]
    assert history["year"] == 2018 and len(history["boards"]) == 2
    song_stage = json.loads((run / "rank_song_sales.json").read_text())
    assert song_stage["rows"] == len(by_query["song_sales"]["page"]["songs"])
    album = by_query["album"]["page"]
    album_stage = json.loads((run / "rank_album_detail.json").read_text())
    assert album_stage["tracks"] == len(album["songs"])
    native = json.loads((run / "rank_playing.json").read_text())["nativePlayer"]
    assert native["state"] == "playing" and native["position"] > 0.2
    assert native["queueLen"] > 1 and native["error"] is None
    album_played = (run / "rank_album_playing.json").exists()
    if album_played:
        native_album = json.loads((run / "rank_album_playing.json").read_text())["nativePlayer"]
        assert native_album["queueLen"] == len(album["songs"])
        assert str(native_album["track"]["id"]) == str(album["songs"][0]["id"])
        assert native_album["state"] == "playing" and native_album["position"] > 0.2
        assert native_album["error"] is None
    result = {"ok": True, "cached_queries": len(by_query), "screenshots": expected,
              "native_rank_playback": True, "native_album_playback": album_played,
              "account_cache_isolated": True, "stages": sorted(p.stem for p in stages)}
    with (root / "independent-audit.json").open("x") as file:
        json.dump(result, file, ensure_ascii=False, indent=2)
        file.write("\n")
    print(json.dumps(result, ensure_ascii=False))


if __name__ == "__main__":
    main()
