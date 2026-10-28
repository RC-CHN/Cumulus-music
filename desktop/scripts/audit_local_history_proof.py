"""Read-only audit of the synthetic local WebView/native history proof."""
import argparse
import json
import pathlib
import sqlite3


def audit(root):
    def read(name):
        return json.loads((root / name).read_text())

    stages = ["local_picker", "local_library", "local_cue", "local_groups",
              "local_history_warning", "local_ncm", "local_reopened-reopened",
              "local_groups-reopened"]
    for stage in stages:
        value = read(stage + ".json")
        assert value["horizontalOverflow"] is False, stage
        assert (root / (stage + ".png")).is_file(), stage
        for row in value["rows"]:
            assert row["playButtonWidth"] <= row["playCellWidth"], stage
            assert row["formatToDurationGap"] >= 8, stage
    warning = read("local_history_warning.json")
    assert warning["historyWarningVisible"] and warning["audioStillAdvancing"]
    for suffix in ["", "-reopened"]:
        complete = read("complete" + suffix + ".json")
        assert complete["ok"] and complete["historyRecorderVerified"]
        assert complete["networkRequests"] == complete["remoteMutations"] == 0
        assert complete["audioOutput"] == "null" and complete["guest"]
        locale = read("native-startup" + suffix + ".json")
        assert locale["afterGuiInitialization"] and locale["numericLocale"] == "C"
        assert locale["messageLocale"] not in ("C", "POSIX")
    first = read("history-local.json")
    assert first == read("history-local-reopened.json")
    assert len(first) == 2 and len({row["id"] for row in first}) == 2
    assert all(row["id"].startswith("local:") and row["origin"] == "local" for row in first)
    assert {row["resource"]["metadata"]["format"] for row in first} == {"flac", "pcm_s16le"}
    cue = next(row for row in first if row["resource"]["local_revision"]["kind"] == "cue")
    assert cue["resource"]["local_revision"]["offset"] == 30
    database = (root / "state/core.sqlite3").resolve()
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as connection:
        rows = connection.execute("SELECT owner,key,value FROM state WHERE kind='play-history-v1'").fetchall()
    assert len(rows) == 1 and rows[0][:2] == ("local-guest", "local:song")
    assert json.loads(rows[0][2]) == first
    return {"ok": True, "webviewStages": len(stages), "screenshots": len(stages),
            "nativeHistoryRecords": 2, "pausedReopenPreservedHistory": True,
            "sqliteMatchesNativeSnapshots": True, "localeVerifiedAfterGuiInitialization": True,
            "historyFailureDidNotStopAudio": True, "audioOutput": "null",
            "networkRequests": 0, "remoteMutations": 0, "physicalFpsVerified": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=pathlib.Path)
    print(json.dumps(audit(parser.parse_args().root), ensure_ascii=False, indent=2))
