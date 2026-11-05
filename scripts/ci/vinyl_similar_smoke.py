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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    parser.add_argument("--related", action="store_true", help="also prove the related playlist lane")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if sys.platform not in {"linux", "win32"} or windows != bool(args.runtime) or (windows and args.screenshots):
        parser.error("Windows needs --runtime; screenshots are Linux only")
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
    counts = {}
    for group, stages in [("t1", expected), ("c", ["control_lyrics"])]:
        directory = root / group
        directory.mkdir()
        script = directory / "probe.js"
        script.write_text("window.__vinylSimilarControl=" + json.dumps(group == "c") + ";\n" +
                          "window.__vinylSimilarRelated=" + json.dumps(args.related) + ";\n" + source,
                          encoding="utf-8")
        run = {**env, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_MOTION_PROOF": "1",
               "CUMULUS_QUEUE_UI_PROOF": "1", "CUMULUS_VINYL_SIMILAR_PROOF": group,
               "CUMULUS_UX_SCRIPT": str(script)}
        if args.related:
            run["CUMULUS_VINYL_SIMILAR_RELATED_PROOF"] = "1"
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
                  stages=counts, screenshots=sum(counts.values()) if args.screenshots else 0,
                  realMusicRequests=0, physicalAudioOutputVerified=False)
    (root / "report.json").write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
