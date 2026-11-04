"""Run vertical-zone entry, navigation, privacy and playback in a real WebView."""

import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime

MENU = "/api/secondary/tab/list"
PRIVACY = "/api/user/setting"
TAB = "/api/zone/tab/get"
CLASSIC = "/api/zone/get"
ALBUM = {"/api/album/v3/detail", "/api/album/v4/detail"}
PLAYLIST = "/api/v6/playlist/detail"
PLAY = "/api/song/enhance/player/url/v1"
MODULES = ["carousel", "horizontal", "ranking", "grid"]
CASES = {
    "full": ("menu_open", "menu", "rock_modules", "album_back", "playlist_back", "playlist_play", "rock_hot", "jazz_fallback"),
    "privacy": ("privacy_gate", "privacy_settings"),
}


def read(path):
    return json.loads(path.read_text(encoding="utf-8"))


def check_case(directory, case, screenshots, windows):
    output = directory / "first"
    complete = read(output / "complete.json")
    assert complete["ok"] and complete["stageCount"] == len(CASES[case])
    calls = complete["verticalZoneCalls"]
    zone_calls = [(call["path"], call["args"]) for call in calls if call["path"] in {TAB, CLASSIC}]
    expected = [(TAB, {"pageCode": "ROCK", "tabKey": "default"})]
    if case == "full":
        expected.extend([
            (TAB, {"pageCode": "ROCK", "tabKey": "1"}),
            (TAB, {"pageCode": "JAZZ", "tabKey": "default"}),
            (CLASSIC, {"pageCode": "JAZZ"}),
        ])
    assert zone_calls == expected, zone_calls
    menu_indices = [idx for idx, call in enumerate(calls) if call["path"] == MENU and call["args"] == {}]
    privacy_indices = [idx for idx, call in enumerate(calls) if call["path"] == PRIVACY and call["args"] == {}]
    first_zone = next(idx for idx, call in enumerate(calls) if call["path"] == TAB)
    assert menu_indices and privacy_indices and menu_indices[0] < first_zone and privacy_indices[0] < first_zone
    assert not any(call["path"] == PRIVACY + "/update" for call in calls), "privacy CTA wrote preferences"
    rows = {}
    for index, name in enumerate(CASES[case], start=1):
        path = output / f"ux_{index:02d}_{name}.json"
        row = rows[name] = read(path)
        assert row["width"] == 920 and row["height"] == 620, (name, row["width"], row["height"])
        if screenshots:
            assert path.with_suffix(".png").is_file()
    if case == "full":
        assert rows["menu_open"]["focus"] == "JAZZ" and rows["menu"]["focus"] == "更多"
        assert rows["rock_modules"]["modules"] == MODULES
        assert rows["rock_hot"]["modules"] == rows["jazz_fallback"]["modules"] == ["grid"]
        assert rows["jazz_fallback"]["zoneTitle"] == "爵士"
        assert rows["album_back"]["focus"] == "专区合成专辑"
        assert rows["playlist_back"]["focus"] == "专区合成歌单 204"
        for name in ("album_back", "playlist_back"):
            row = rows[name]
            assert row["expected"]["top"] >= 60 and row["expected"]["left"] >= 75
            assert all(abs(row["expected"][axis] - row["actual"][axis]) <= 2 for axis in ("top", "left"))
        assert any(call["path"] in ALBUM and str(call["args"]["id"]) == "7" for call in calls), calls
        assert any(call["path"] == PLAYLIST and str(call["args"]["id"]) == "204" for call in calls), calls
        authorizations = [call["args"] for call in calls if call["path"] == PLAY]
        # The current track is required. The native actor may also prefetch
        # the sole next track before the captured run stops.
        assert 1 <= len(authorizations) <= 2, authorizations
        for expected_id, authorization in zip(("81", "84"), authorizations):
            assert authorization.get("trialMode") == 121, authorizations
            assert list(map(str, json.loads(authorization["ids"]))) == [expected_id], authorizations
        playback = rows["playlist_play"]
        player = playback["nativePlayer"]
        assert playback["tracks"] == ["81", "84"] and player["track"]["id"] == "81"
        assert playback["playId"] == player["playId"] and player["position"] > 0.15
        assert player["state"] == "playing" and playback["audioFactoryCount"] == 1
    else:
        assert rows["privacy_gate"]["modules"] == MODULES
        assert rows["privacy_settings"]["focus"] == "settings-privacy"
        assert not any(call["path"] == PLAY for call in calls)
    if not windows:
        locale = read(output / "locale.json")
        assert locale["numeric"] == "C"
        assert all(locale[key].casefold().replace("-", "") == "en_us.utf8" for key in ("messages", "characters"))
    return {"states": list(rows), "zoneReads": zone_calls}


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
    base_env = {key: value for key, value in os.environ.items()
                if not key.upper().startswith(("CUMULUS_", "WEBVIEW2_"))
                and key.upper() != "MPV_DLL_DIR_CUMULUS"}
    media = None
    if windows:
        stage, media = windows_runtime(binary, args.runtime)
        base_env["MPV_DLL_DIR_CUMULUS"] = str(stage)
        profiles = ("APPDATA", "LOCALAPPDATA", "TEMP", "TMP")
    else:
        base_env["LC_ALL"] = "en_US.UTF-8"
        profiles = ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR")
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    probe = Path(__file__).resolve().parents[2] / "desktop/examples/vertical_zone_ui_probe.js"
    script_source = probe.read_text(encoding="utf-8")
    result = {}
    for case in CASES:
        directory = root / case
        directory.mkdir()
        script = directory / "probe.js"
        script.write_text(f"window.__verticalZonePrivacy={str(case == 'privacy').lower()};\n" + script_source,
                          encoding="utf-8")
        env = {**base_env, "CUMULUS_PROOF_DIR": str(directory), "CUMULUS_MOTION_PROOF": "1",
               "CUMULUS_VERTICAL_ZONE_PROOF": case, "CUMULUS_UX_SCRIPT": str(script)}
        for key in profiles:
            isolated = directory / "profile" / key
            isolated.mkdir(parents=True, mode=0o700)
            env[key] = str(isolated)
        if args.frontend_dist:
            env["CUMULUS_PROOF_FRONTEND_DIST"] = str(args.frontend_dist.resolve(strict=True))
        output = directory / "first"
        with (directory / "host.log").open("x", encoding="utf-8") as log:
            code = launch(binary, env, log, output, args.screenshots)
        failed = output / "failed.json"
        assert not failed.exists(), failed.read_text(encoding="utf-8") if failed.exists() else ""
        assert code == 0, (case, code, directory)
        result[case] = check_case(directory, case, args.screenshots, windows)
    summary = dict(ok=True, actualWebView=True, nativeLibmpvNull=True, windowsMediaRuntime=media,
                   stateCount=sum(map(len, CASES.values())),
                   screenshots=sum(map(len, CASES.values())) if args.screenshots else 0,
                   realMusicRequests=0, realAccountWrites=0, cases=result)
    (root / "audit.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(summary))


if __name__ == "__main__":
    main()
