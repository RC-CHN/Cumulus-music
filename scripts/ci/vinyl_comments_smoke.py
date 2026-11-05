"""Verify the original vinyl comment mode and stock route in a real WebView."""

import argparse
import json
import os
from pathlib import Path
import sqlite3
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime

COUNT = "/api/resource/commentInfo/list"
LATEST = "/api/v1/resource/comments/R_SO_4_1"
HOT = "/api/v1/resource/hotcomments/R_SO_4_1"
SECOND = "/api/v1/resource/comments/R_SO_4_2"
ADD = "/api/resource/comments/add"
REPLY = "/api/v1/resource/comments/reply"
LIKE = "/api/v1/comment/like"
UNLIKE = "/api/v1/comment/unlike"
MUTATIONS = {ADD, REPLY, LIKE, UNLIKE, "/api/resource/comments/delete",
             "/api/report/reportcomment"}
COMPOSER_STAGES = (
    "composer_zero", "cancelled", "utf16_rejected", "published", "stale_open", "stale_closed",
    "reply_target", "reply_confirmed", "reply_uncertain", "independent_recovery",
    "independent_unresolved",
)

LIKES_STAGES = ("like_initial", "like_confirmed", "unlike_uncertain", "unlike_reconciled")
REPORT = "/api/report/reportcomment"
REPORT_STAGES = ("report_cancelled", "report_preset_accepted", "report_other_uncertain", "report_reopened")



def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def check_composer(rows, complete):
    def writes(stage):
        return [call for call in stage["vinylCommentsCalls"] if call["path"] in MUTATIONS]

    def lookups(stage, path):
        return [call["args"]["commentId"] for call in stage["vinylCommentsCalls"]
                if call["path"] == path and call["args"]["limit"] == 1]

    expected = [
        dict(path=ADD, args=dict(threadId="R_SO_4_2", content="  零评论发布 🎵\n保留空白  ",
                                 resourceType=0, resourceId=0, expressionPicId=-1, bubbleId=-1)),
        dict(path=REPLY, args=dict(threadId="R_SO_4_1", commentId="1", content="  回复原评论 🎧 ",
                                   resourceType=0, resourceId=0)),
        dict(path=REPLY, args=dict(threadId="R_SO_4_1", commentId="1", content="合成回复已送达，回包丢失",
                                   resourceType=0, resourceId=0)),
    ]
    assert complete["realAccountWrites"] == complete["realMusicRequests"] == 0
    for index, key in enumerate(COMPOSER_STAGES):
        count = 0 if index < 3 else 1 if index < 7 else 2 if index < 8 else 3
        assert writes(rows[key]) == expected[:count], (key, writes(rows[key]))
    assert writes(complete) == expected
    zero = rows["composer_zero"]
    assert zero["tab"] == "comments" and zero["trackId"] == "2" and zero["count"] == 0
    assert not any(call["path"] == SECOND for call in zero["vinylCommentsCalls"])
    assert rows["cancelled"]["count"] == 0
    assert rows["utf16_rejected"]["units"] == 1001 and rows["utf16_rejected"]["disabled"]
    published = rows["published"]
    assert (published["trackId"], published["count"], published["latest"], published["confirmed"]) == (
        "2", 1, 1, "9001",
    )
    assert lookups(published, SECOND) == ["9001", "9001"]
    assert rows["stale_open"]["playId"] == published["playId"]
    closed = rows["stale_closed"]
    assert closed["trackId"] == "1" and closed["playId"] != published["playId"] and closed["count"] == 63
    assert rows["reply_target"]["placeholder"] == "回复 合成作者: "
    confirmed = rows["reply_confirmed"]
    assert confirmed["count"] == 64 and confirmed["confirmed"] == "9002"
    assert lookups(confirmed, LATEST) == ["1", "1", "9002", "9002"]
    uncertain = rows["reply_uncertain"]
    assert uncertain["tab"] == "comments" and uncertain["count"] == 64
    assert lookups(uncertain, LATEST) == ["1", "1", "9002", "9002", "1", "1"]
    for key in ("independent_recovery", "independent_unresolved"):
        assert rows[key]["tab"] is None and rows[key]["pending"] == uncertain["pending"]
        assert rows[key]["playId"] == confirmed["playId"]
    assert lookups(complete, LATEST) == lookups(uncertain, LATEST)
    assert any(call["path"] == LATEST and call["args"]["limit"] == 20
               for call in rows["independent_recovery"]["vinylCommentsCalls"])


def check_likes(rows, complete, directory):
    page = dict(limit=60, offset=0, beforeTime=0, commentId="0",
                compareUserLocation=False, composeConcert=False, markReplied=False,
                forceFlatComment=False, showInner=False)
    target = dict(page, limit=1, commentId="1")
    list_read = dict(path=LATEST, args=page, lane="bound_read")
    target_read = dict(path=LATEST, args=target, lane="bound_read")
    like = dict(path=LIKE, args=dict(threadId="R_SO_4_1", commentId="1"), lane="bound_write")
    unlike = dict(path=UNLIKE, args=like["args"], lane="bound_write")
    calls = [dict(path=COUNT, args=dict(resourceType=4, resourceIds='["1"]')), list_read]
    expected = [
        calls, calls + [target_read, like, target_read, target_read],
        calls + [target_read, like, target_read, target_read, target_read, unlike],
        calls + [target_read, like, target_read, target_read, target_read, unlike,
                 target_read, target_read],
    ]
    assert complete["realAccountWrites"] == complete["realMusicRequests"] == 0
    first = rows[LIKES_STAGES[0]]
    assert first["scroll"] >= 35 and first["nativePlayer"]["entryId"]
    for key, wire, pressed, count in zip(LIKES_STAGES, expected, (False, True, True, False),
                                         ("", "1", "1", "")):
        row = rows[key]
        assert row["vinylCommentsCalls"] == wire, (key, row["vinylCommentsCalls"])
        assert row["pressed"] is pressed and row["shownCount"] == count
        assert row["tab"] == "comments" and row["trackId"] == "1"
        assert row["latest"] == 60 and row["hot"] == 1
        assert row["playId"] == first["playId"]
        assert row["nativePlayer"]["queueRevision"] == first["nativePlayer"]["queueRevision"]
        assert row["nativePlayer"]["entryId"] == first["nativePlayer"]["entryId"]
        assert abs(row["nativePlayer"]["position"] - first["nativePlayer"]["position"]) < .1
        assert abs(row["scroll"] - first["scroll"]) < 1
    assert rows["unlike_uncertain"]["phase"] == "uncertain"
    assert complete["vinylCommentsCalls"] == expected[-1]

    database = (directory / "state/core.sqlite3").resolve(strict=True)
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True) as conn:
        journal = conn.execute(
            "SELECT owner,key,value FROM state WHERE kind='comment-likes-v1'"
        ).fetchall()
    assert len(journal) == 1 and journal[0][:2] == ("2", "R_SO_4_1:1")
    receipt = json.loads(journal[0][2])
    assert receipt["phase"] == "confirmed" and receipt["before"] and not receipt["desired"]
    assert not receipt["acknowledged"], "readback must not fabricate the lost acknowledgement"
    assert receipt["observed"]["liked"] is False


def check_reports(rows, complete):
    def mutations(record):
        return [call for call in record["vinylCommentsCalls"] if call["path"] in MUTATIONS]

    wanted = [
        {"path": REPORT, "lane": "bound_write", "args": {
            "threadId": "R_SO_4_1", "commentId": "1", "reason": "段子或者无意义的评论"}},
        {"path": REPORT, "lane": "bound_write", "args": {
            "threadId": "R_SO_4_1", "commentId": "3", "reason": "  合成原因原样发送  "}},
    ]
    assert not mutations(rows["report_cancelled"])
    assert mutations(rows["report_preset_accepted"]) == wanted[:1]
    assert mutations(rows["report_other_uncertain"]) == mutations(rows["report_reopened"]) == mutations(complete) == wanted
    bound_targets = [call["args"]["commentId"] for call in complete["vinylCommentsCalls"]
                     if call["path"] == LATEST and call["args"]["limit"] == 1]
    assert bound_targets == ["1", "3"], bound_targets
    for name in REPORT_STAGES:
        assert rows[name]["tab"] == "comments" and rows[name]["scroll"] == rows[REPORT_STAGES[0]]["scroll"]
        assert rows[name]["playId"] == rows[REPORT_STAGES[0]]["playId"]
    assert rows["report_preset_accepted"]["outcome"] == "举报成功"
    assert rows["report_other_uncertain"]["outcome"] == rows["report_reopened"]["outcome"]
    assert "请勿重复提交" in rows["report_reopened"]["outcome"]
    assert complete["realMusicRequests"] == complete["realAccountWrites"] == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    modes = parser.add_mutually_exclusive_group()
    modes.add_argument("--composer", action="store_true", help="run the sealed composer write scenario")
    modes.add_argument("--likes", action="store_true", help="run sealed inline like/unlike and reconciliation")
    modes.add_argument("--reports", action="store_true", help="run sealed inline report and durable receipt")
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
    examples = Path(__file__).resolve().parents[2] / "desktop/examples"
    source = (examples / "vinyl_comments_ui_probe.js").read_text(encoding="utf-8")
    expected = {
        "t1": ("standard_before_read", "embedded_first", "embedded_append", "return_focus", "hot_route", "zero_count"),
        "c": ("control_route",),
    }
    if args.composer:
        source = (examples / "vinyl_composer_ui_probe.js").read_text(encoding="utf-8") + "\n" + source
        expected = {"t1": COMPOSER_STAGES}
    elif args.likes:
        source = (examples / "vinyl_likes_ui_probe.js").read_text(encoding="utf-8") + "\n" + source
        expected = {"t1": LIKES_STAGES}
    elif args.reports:
        source = (examples / "vinyl_report_ui_probe.js").read_text(encoding="utf-8") + "\n" + source
        expected = {"t1": REPORT_STAGES}
    summary = {}
    for group, names in expected.items():
        directory = root / group
        directory.mkdir()
        script = directory / "probe.js"
        script.write_text(f"window.__vinylCommentControl={json.dumps(group == 'c')};\n" + source,
                          encoding="utf-8")
        run = {**env, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_MOTION_PROOF": "1",
               "CUMULUS_VINYL_COMMENTS_PROOF": group, "CUMULUS_UX_SCRIPT": str(script)}
        if args.composer:
            run["CUMULUS_VINYL_COMPOSER_PROOF"] = "1"
        elif args.likes:
            run["CUMULUS_VINYL_LIKES_PROOF"] = "1"
        elif args.reports:
            run["CUMULUS_VINYL_REPORTS_PROOF"] = "1"
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
        assert status == 0 and complete["stageCount"] == len(names)
        rows = {path.stem.split("_", 2)[2]: read(path) for path in sorted(output.glob("ux_*.json"))}
        assert tuple(rows) == names
        for row in rows.values():
            assert row["nativePlayer"]["playId"] == row["playId"] and row["nativePlayer"]["state"] == "paused"
        if args.composer:
            check_composer(rows, complete)
        elif args.likes:
            check_likes(rows, complete, directory)
        elif args.reports:
            check_reports(rows, complete)
        elif group == "t1":
            assert [call["path"] for call in rows["standard_before_read"]["vinylCommentsCalls"]] == [COUNT]
            calls = rows["zero_count"]["vinylCommentsCalls"]
            assert [call["path"] for call in calls] == [COUNT, LATEST, LATEST, HOT, COUNT]
            for call, offset in zip(calls[1:3], [0, 60]):
                assert call["args"] == dict(limit=60, offset=offset, beforeTime=0, commentId="0",
                    compareUserLocation=False, composeConcert=False, markReplied=False,
                    forceFlatComment=False, showInner=False)
            assert calls[0]["args"] == {"resourceType": 4, "resourceIds": '["1"]'}
            assert calls[-1]["args"] == {"resourceType": 4, "resourceIds": '["2"]'}
            assert rows["embedded_first"]["latest"] == 60 and rows["embedded_first"]["hot"] == 5
            assert rows["embedded_append"]["latest"] == 63 and rows["embedded_append"]["hot"] == 5
            assert rows["return_focus"]["tab"] == "lyrics" and rows["hot_route"]["tab"] is None
            assert rows["zero_count"]["tab"] == "comments" and rows["zero_count"]["trackId"] == "2"
            assert len({row["playId"] for key, row in rows.items() if key != "zero_count"}) == 1
        else:
            calls = rows["control_route"]["vinylCommentsCalls"]
            assert [call["path"] for call in calls] == [COUNT, LATEST]
            assert calls[1]["args"]["limit"] == 20 and rows["control_route"]["tab"] is None
        if not windows:
            locale = read(output / "locale.json")
            assert locale["numeric"] == "C"
            assert all(locale[key].casefold().replace("-", "") == "en_us.utf8"
                       for key in ("messages", "characters"))
        if args.screenshots:
            assert len(list(output.glob("ux_*.png"))) == len(names)
        summary[group] = list(rows)
    report = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, states=summary,
                  realMusicRequests=0, realAccountWrites=0)
    if args.likes:
        report.update(syntheticLikeWrites=2, lostUnlikeAcknowledgement=True,
                      readOnlyReconciled=True, preservedPlaybackAndScroll=True)
    if args.reports:
        report.update(syntheticReportWrites=2, lostReportAcknowledgement=True,
                      reusedDurableReceipt=True, preservedPlaybackAndScroll=True)
    (root / "audit.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
