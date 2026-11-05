"""Prove mounted zone FM previews and explicit reuse in the actual desktop."""

import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime

RECOMMEND = "/api/tag/tab/fm/recommends"
DETAIL = "/api/v3/song/detail"
URL = "/api/song/enhance/player/url/v1"
EXPLICIT_READS = {"/api/music/dislike/get", "/api/v3/radio/trash/get", "/api/music-vip-membership/client/vip/info"}
NAMES = ("ux_01_before_mount", "ux_02_passive_art", "ux_03_explicit_play")


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def verify(output, screenshots, windows):
    complete = read(output / "complete.json")
    assert complete["ok"] and complete["stageCount"] == len(NAMES)
    before, preview, explicit = (read(output / f"{name}.json") for name in NAMES)
    for row in (before, preview):
        assert row["nativePlayer"]["state"] == "idle" and not row["nativePlayer"]["fm"]
        assert row["nativePlayer"]["playId"] is row["playId"] is None
    assert before["audioFactoryCount"] == preview["audioFactoryCount"]
    pre = before["zoneFmPreview"]["calls"]
    assert all(call["path"] in EXPLICIT_READS for call in pre), pre
    calls = preview["zoneFmPreview"]["calls"]
    assert calls[:len(pre)] == pre
    passive = calls[len(pre):]
    assert len(passive) == 8 and {call["path"] for call in passive} == {RECOMMEND, DETAIL}, passive
    assert sorted(call["args"]["ruleId"] for call in passive if call["path"] == RECOMMEND) == ["11", "12", "13", "14"]
    details = [call["args"] for call in passive if call["path"] == DETAIL]
    assert all(item["trialMode"] == -1 for item in details)
    assert sorted(item["c"] for item in details) == sorted([
        '[{"id":81,"v":5},{"id":"84","v":0}]', '[{"id":91,"v":0}]',
        '[{"id":92,"v":0}]', '[{"id":93,"v":0}]',
    ])
    assert 2 <= preview["zoneFmPreview"]["peak"] <= 3 and preview["zoneFmPreview"]["active"] == 0
    assert preview["album"] == preview["duplicate"] == "https://p1.music.126.net/synthetic-zone-album-81.png?param=320y320"
    assert preview["fallback"] == "https://p1.music.126.net/synthetic-zone-card-12.png?param=320y320"
    assert preview["blockedArtwork"] > 0
    after = complete["zoneFmPreview"]["calls"]
    assert after[:len(calls)] == calls
    assert {call["path"] for call in after[len(calls):]} <= EXPLICIT_READS | {URL}, after
    urls = [call["args"] for call in after[len(calls):] if call["path"] == URL]
    assert 1 <= len(urls) <= 2 and all(call["trialMode"] == 121 for call in urls), urls
    assert [json.loads(call["ids"]) for call in urls] == [["81"], ["84"]][:len(urls)], urls
    player = explicit["nativePlayer"]
    assert player["fm"] and player["state"] == "playing" and player["position"] > 0.15
    assert player["track"]["id"] == "81" and explicit["tracks"] == ["81", "84"]
    assert explicit["playId"] and explicit["playId"] == player["playId"]
    if screenshots:
        assert all((output / f"{name}.png").is_file() for name in NAMES)
    if not windows:
        locale = read(output / "locale.json")
        assert locale["numeric"] == "C"
        assert all(locale[key].casefold().replace("-", "") == "en_us.utf8" for key in ("messages", "characters"))
    return {"states": list(NAMES), "passiveCalls": len(passive), "peakReads": preview["zoneFmPreview"]["peak"],
            "playTrack": "81", "explicitTrialMode": 121}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--frontend-dist", type=Path)
    parser.add_argument("--runtime", type=Path)
    parser.add_argument("--screenshots", action="store_true")
    args = parser.parse_args()
    windows = sys.platform == "win32"
    if sys.platform not in {"linux", "win32"} or windows != bool(args.runtime) or windows and args.screenshots:
        parser.error("Windows requires --runtime and Linux alone permits screenshots")
    binary = args.binary.resolve(strict=True)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_")) and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    media = None
    if windows:
        stage, media = windows_runtime(binary, args.runtime)
        env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profiles = ("APPDATA", "LOCALAPPDATA", "TEMP", "TMP")
    else:
        env["LC_ALL"] = "en_US.UTF-8"
        profiles = ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR")
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    source = Path(__file__).resolve().parents[2] / "desktop/examples/zone_fm_preview_ui_probe.js"
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF="1", CUMULUS_VERTICAL_ZONE_PROOF="fm-preview",
               CUMULUS_UX_SCRIPT=str(source))
    for key in profiles:
        isolated = root / "profile" / key
        isolated.mkdir(parents=True, mode=0o700)
        env[key] = str(isolated)
    if args.frontend_dist:
        env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
    output = root / "first"
    with (root / "host.log").open("x", encoding="utf-8") as log:
        code = launch(binary, env, log, output, args.screenshots)
    failed = output / "failed.json"
    assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
    assert code == 0, (code, root)
    report = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, windowsMediaRuntime=media,
                  coverDomVerified=True, coverImageBytesVerified=False, syntheticCdnBlocked=True,
                  realMusicRequests=0, realAccountWrites=0,
                  **verify(output, args.screenshots, windows))
    (root / "audit.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
