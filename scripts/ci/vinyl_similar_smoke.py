"""Real WebView vinyl recommendations, native playback and sealed service responses."""
import argparse
import json
import os
from pathlib import Path
import sys

from playback_power_smoke import windows_runtime
from window_manager import launch


SONGS = "/api/v1/discovery/simiSong"
PLAYLISTS = "/api/discovery/simiPlaylist"
WIKI = "/api/music/wiki/home/song/desktop/get"
EXP = "PH-PC-playPage-recommendMore-new"
BASE_STAGES = ("before_read", "read_failed", "read_retried", "title_queued", "cover_queued",
               "row_replaced", "preference_add", "row_added", "late_discarded")


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def calls(record, lane):
    return [call for call in record["vinylSimilarFixture"]["calls"] if call["lane"] == lane]


def song_reads(record):
    values = calls(record, "songs")
    for call in values:
        seed = call["args"]["songid"]
        assert call == dict(lane="songs", path=SONGS, args=dict(songid=seed)) and seed.isdecimal()
    return [call["args"]["songid"] for call in values]


def playlist_reads(record):
    values = calls(record, "playlists")
    for call in values:
        seed = call["args"]["songid"]
        assert call == dict(lane="playlists", path=PLAYLISTS, args=dict(songid=seed)) and seed.isdecimal()
    return [call["args"]["songid"] for call in values]


def wiki_reads(record):
    values = calls(record, "wiki")
    for call in values:
        seed = call["songId"]
        assert call == dict(lane="wiki", path=WIKI, songId=seed, signatureVerified=True)
        assert seed.isdecimal()
    return [call["songId"] for call in values]


def ids(row):
    return [entry["track"]["id"] for entry in row["nativeQueue"]["tracks"]]


def preserved(before, after):
    old = [entry["entryId"] for entry in before["nativeQueue"]["tracks"]]
    kept = [entry["entryId"] for entry in after["nativeQueue"]["tracks"] if entry["entryId"] in old]
    assert old == kept


def assert_play(row, song):
    native = row["nativePlayer"]
    queue = row["nativeQueue"]
    assert native["state"] == "paused" and native["track"]["id"] == song == row["trackId"]
    assert native["playId"] == row["playId"] and row["playedPosition"] > .18
    assert native["queueRevision"] == row["queueRevision"] == queue["revision"]
    assert native["entryId"] == queue["current"]
    assert row["audioFactoryCount"] >= 1
    assert queue["current"] in {entry["entryId"] for entry in queue["tracks"] if entry["track"]["id"] == song}
    # The stock player can authorize the next queue item before selecting it.
    # Require its source authorization by this native stage, rather than a
    # redundant URL request after each gesture.
    assert song in {str(identifier) for call in calls(row, "audio") for identifier in call["ids"]}


def audit_treatment(rows, complete, related):
    baseline = rows["before_read"]
    assert baseline["tab"] == "lyrics" and not song_reads(baseline) and not playlist_reads(baseline)
    assert rows["read_failed"]["songState"] == "error"
    assert song_reads(rows["read_failed"]) == ["1"]
    assert rows["read_retried"]["songState"] == "ready" and rows["read_retried"]["songIds"] == ["51", "52", "53"]
    assert song_reads(rows["read_retried"]) == ["1", "1"]
    assert_play(baseline, "1")
    assert_play(rows["read_failed"], "1")
    assert_play(rows["read_retried"], "1")
    assert baseline["playId"] == rows["read_failed"]["playId"] == rows["read_retried"]["playId"]
    title, cover = rows["title_queued"], rows["cover_queued"]
    assert_play(title, "51")
    assert_play(cover, "55")
    assert ids(title) == ["1", "51", "52", "53", *ids(baseline)[1:]]
    assert ids(cover) == ["1", "51", "55", *ids(title)[2:]]
    preserved(baseline, title)
    preserved(title, cover)
    assert title["preference"] is True and cover["preference"] is True
    replaced, setting, added, late = [rows[key] for key in ("row_replaced", "preference_add", "row_added", "late_discarded")]
    assert_play(replaced, "58")
    assert_play(setting, "58")
    assert_play(added, "61")
    assert_play(late, "59")
    assert replaced["preference"] is True and setting["preference"] is added["preference"] is False
    assert ids(replaced) == ids(setting) == ["57", "58", "59"]
    assert replaced["playId"] == setting["playId"] and replaced["queueRevision"] == setting["queueRevision"]
    assert ids(added) == ids(late) == ["57", "58", "61", "59"]
    preserved(replaced, added)
    assert added["vinylSimilarFixture"]["holding"] and not added["vinylSimilarFixture"]["released"]
    assert late["vinylSimilarFixture"]["released"] and late["vinylSimilarFixture"]["heldReturned"]
    assert late["songState"] == "ready" and late["songIds"] == ["84", "85", "86"]
    seeds = song_reads(complete)
    assert set(seeds) == {"1", "51", "55", "58", "61", "59"}
    assert seeds.index("61") < seeds.index("59")
    if related:
        first_expanded = rows["related_first_expanded"]
        expanded, route = rows["related_expanded"], rows["playlist_route"]
        visible = [str(identifier) for identifier in range(101, 109)]
        assert rows["read_failed"]["playlistIds"] == rows["read_retried"]["playlistIds"] == visible[:6]
        assert first_expanded["playlistIds"] == expanded["playlistIds"] == visible
        assert late["playlistIds"] == visible[:6], "expansion must reset on a different song"
        assert playlist_reads(first_expanded) == playlist_reads(rows["read_retried"])
        assert playlist_reads(expanded) == playlist_reads(late), "expand must not refetch"
        assert set(playlist_reads(complete)) == set(seeds)
        assert_play(route, "59")
        assert route["openedPlaylist"] == "108" and route["tab"] is None
        assert [str(call["id"]) for call in calls(route, "detail")][-1] == "108"
    else:
        assert not playlist_reads(complete)


def wiki_metas(seed):
    return [["演唱", "合成百科 " + seed], ["曲风", "轻音乐、钢琴"], ["原样文本", "<b>合成内容</b>"]]


def audit_wiki_treatment(rows, complete):
    baseline = rows["before_read"]
    failed, retried = rows["wiki_failed"], rows["wiki_retried"]
    left, reopened = rows["wiki_left_for_lyrics"], rows["wiki_same_seed_reloaded"]
    assert baseline["tab"] == "lyrics" and baseline["hasSimilarTab"] and baseline["hasWikiTab"]
    assert not wiki_reads(baseline)
    for stage in (failed, retried, left, reopened):
        assert_play(stage, "1")
        assert stage["playId"] == baseline["playId"] and stage["queueRevision"] == baseline["queueRevision"]
        assert not song_reads(stage) and not playlist_reads(stage)
        assert not stage["songIds"] and not stage["playlistIds"]
    assert failed["tab"] == retried["tab"] == reopened["tab"] == "wiki"
    assert failed["wikiState"] == "error" and wiki_reads(failed) == ["1"]
    assert retried["wikiState"] == "ready" and retried["wikiMetas"] == wiki_metas("1")
    assert left["tab"] == "lyrics" and left["wikiState"] is None and not left["wikiMetas"]
    assert left["focusedDetailTab"] == left["selectedDetailTab"] == "lyrics"
    assert wiki_reads(retried) == wiki_reads(left) == ["1", "1"]
    assert reopened["wikiState"] == "ready" and reopened["wikiMetas"] == wiki_metas("1")
    assert reopened["focusedDetailTab"] == reopened["selectedDetailTab"] == "wiki"
    assert wiki_reads(reopened) == ["1", "1", "1"]
    assert wiki_reads(rows["playlist_route"]) == wiki_reads(reopened), "standalone recommendations must not request Wiki"

    held, late = rows["wiki_old_pending"], rows["wiki_late_discarded"]
    assert_play(held, "59")
    assert_play(late, "61")
    assert held["tab"] == late["tab"] == "wiki"
    assert held["wikiState"] == "loading" and held["vinylSimilarFixture"]["wikiHolding"]
    assert not held["vinylSimilarFixture"]["wikiReleased"] and wiki_reads(held) == ["1", "1", "1", "59"]
    assert late["wikiState"] == "ready" and late["wikiMetas"] == wiki_metas("61")
    assert late["vinylSimilarFixture"]["wikiReleased"] and late["vinylSimilarFixture"]["wikiHeldReturned"]
    assert wiki_reads(complete) == ["1", "1", "1", "59", "61"]
    # Reopening vinyl restores the previously selected Similar tab. Its two
    # scoped reads finish before the user changes tabs to Wiki.
    assert song_reads(held) == [*song_reads(rows["playlist_route"]), "59"]
    assert playlist_reads(held) == [*playlist_reads(rows["playlist_route"]), "59"]
    assert song_reads(complete) == song_reads(held)
    assert playlist_reads(complete) == playlist_reads(held)


def audit_wiki_control(rows, complete):
    baseline, failed, retried, expanded, played = [rows[key] for key in (
        "control_lyrics", "wiki_failed", "wiki_retried", "wiki_playlists_expanded", "wiki_title_queued")]
    assert baseline["tab"] == "lyrics" and baseline["hasWikiTab"] and not baseline["hasSimilarTab"]
    assert not song_reads(baseline) and not playlist_reads(baseline) and not wiki_reads(baseline)
    ids_visible = [str(identifier) for identifier in range(101, 109)]
    for stage in (failed, retried, expanded):
        assert_play(stage, "1")
        assert stage["playId"] == baseline["playId"] and stage["tab"] == "wiki"
        assert stage["wikiOrder"] == ["wiki", "songs", "playlists"]
        assert stage["songIds"] == ["51", "52", "53", "54", "55"]
        assert song_reads(stage) == playlist_reads(stage) == ["1"]
    assert failed["wikiState"] == "error" and wiki_reads(failed) == ["1"]
    assert retried["wikiState"] == "ready" and retried["wikiMetas"] == wiki_metas("1")
    assert wiki_reads(retried) == wiki_reads(expanded) == ["1", "1"]
    assert failed["playlistIds"] == retried["playlistIds"] == ids_visible[:6]
    assert expanded["playlistIds"] == ids_visible

    assert_play(played, "51")
    assert played["tab"] == "wiki" and played["wikiOrder"] == ["wiki", "songs", "playlists"]
    assert played["wikiMetas"] == wiki_metas("51") and played["playlistIds"] == ids_visible[:6]
    assert ids(played) == ["1", "51", "52", "53", "54", "55", *ids(baseline)[1:]]
    preserved(baseline, played)
    assert wiki_reads(complete) == ["1", "1", "51"]
    assert song_reads(complete) == playlist_reads(complete) == ["1", "51"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    parser.add_argument("--related", action="store_true", help="also prove the related playlist lane")
    parser.add_argument("--wiki", action="store_true", help="also prove both encyclopedia placements")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if sys.platform not in {"linux", "win32"} or windows != bool(args.runtime) or (windows and args.screenshots):
        parser.error("Windows needs --runtime; screenshots are Linux only")
    if args.wiki and not args.related:
        parser.error("--wiki requires --related for the full control card grouping")
    binary = args.binary.resolve(strict=True)
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_")) and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    if windows:
        stage, _ = windows_runtime(binary, args.runtime)
        env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profiles = ("APPDATA", "LOCALAPPDATA", "TEMP", "TMP")
    else:
        env["LC_ALL"] = "en_US.UTF-8"
        profiles = ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR")
    if args.frontend_dist:
        env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
    source = (Path(__file__).resolve().parents[2] / "desktop/examples/vinyl_similar_ui_probe.js").read_text(encoding="utf-8")
    expected = list(BASE_STAGES)
    if args.related:
        expected.insert(expected.index("title_queued"), "related_first_expanded")
        expected.extend(("related_expanded", "playlist_route"))
    control = ["control_lyrics"]
    if args.wiki:
        expected[1:1] = ["wiki_failed", "wiki_retried", "wiki_left_for_lyrics", "wiki_same_seed_reloaded"]
        expected.extend(("wiki_old_pending", "wiki_late_discarded"))
        control.extend(("wiki_failed", "wiki_retried", "wiki_playlists_expanded", "wiki_title_queued"))
    counts = {}
    for group, stages in [("t1", expected), ("c", control)]:
        directory = root / group
        directory.mkdir()
        script = directory / "probe.js"
        script.write_text("window.__vinylSimilarControl=" + json.dumps(group == "c") + ";\n" +
                          "window.__vinylSimilarRelated=" + json.dumps(args.related) + ";\n" +
                          "window.__vinylWikiProof=" + json.dumps(args.wiki) + ";\n" + source,
                          encoding="utf-8")
        run = {**env, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_MOTION_PROOF": "1",
               "CUMULUS_QUEUE_UI_PROOF": "1", "CUMULUS_VINYL_SIMILAR_PROOF": group,
               "CUMULUS_UX_SCRIPT": str(script)}
        if args.related:
            run["CUMULUS_VINYL_SIMILAR_RELATED_PROOF"] = "1"
        if args.wiki:
            run["CUMULUS_VINYL_WIKI_PROOF"] = "1"
        for key in profiles:
            isolated = directory / key
            isolated.mkdir(mode=0o700)
            run[key] = str(isolated)
        output = directory / "first"
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            status = launch(binary, run, log, output, args.screenshots)
        failed = output / "failed.json"
        assert not failed.exists(), read(failed) if failed.exists() else None
        complete = read(output / "complete.json")
        assert status == 0 and complete["ok"] and complete["stageCount"] == len(stages)
        assert complete["realMusicRequests"] == 0
        assert not calls(complete, "unbound")
        assert calls(complete, "assignment") and all(
            row == dict(lane="assignment", name=EXP, group=group)
            for row in calls(complete, "assignment"))
        rows = {path.stem.split("_", 2)[2]: read(path) for path in sorted(output.glob("ux_*.json"))}
        assert list(rows) == stages
        if group == "t1":
            audit_treatment(rows, complete, args.related)
            if args.wiki:
                audit_wiki_treatment(rows, complete)
        elif args.wiki:
            audit_wiki_control(rows, complete)
        else:
            assert not song_reads(complete) and not playlist_reads(complete)
            assert_play(rows["control_lyrics"], "1")
            assert rows["control_lyrics"]["tab"] == "lyrics"
        if not windows:
            locale = read(output / "locale.json")
            assert locale["numeric"] == "C"
            assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                       for key in ("messages", "characters"))
        if args.screenshots:
            assert len(list(output.glob("ux_*.png"))) == len(stages)
        counts[group] = len(stages)
    result = dict(ok=True, actualWebView=True, syntheticNativeAudio=True, relatedPlaylists=args.related,
                  wiki=args.wiki,
                  stages=counts, screenshots=sum(counts.values()) if args.screenshots else 0,
                  realMusicRequests=0, physicalAudioOutputVerified=False)
    (root / "report.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
