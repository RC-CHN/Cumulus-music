#!/usr/bin/env python3
"""Independently verify synthetic auth UI evidence and persisted SQLite state.

Only accepts output of auth_ui_probe. Does not print Cookie or refresh values.
"""
import argparse
import hashlib
import json
from pathlib import Path
import sqlite3


def rows(path):
    with sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True) as db:
        return [(owner, kind, key, json.loads(value)) for owner, kind, key, value
                in db.execute("SELECT owner,kind,key,value FROM state")]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("--without-screenshots", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    # Require synthetic-run markers before inspecting any credential storage.
    first = json.loads((root / "first/complete.json").read_text(encoding="utf-8"))
    assert first["ok"] and first["networkRequests"] == first["realAccountMutations"] == 0
    assert first["expectNickname"] == "短信验证账号"
    expected = {"2": "邮箱验证账号", "3": "二次验证账号", "4": "安全问题账号", "5": "短信验证账号"}
    registry = rows(root / "state/core.sqlite3")
    saved = {key: value for owner, kind, key, value in registry
             if owner == "transport" and kind == "saved-profiles-v1"}
    assert {uid: value["nickname"] for uid, value in saved.items()} == expected
    active = next(value for owner, kind, key, value in registry
                  if (owner, kind, key) == ("transport", "profiles", "active"))
    assert active["session_id"] == saved["5"]["session_id"] and active["auto_login"] is False
    assert saved["5"]["auto_login"] is False
    check = {}
    for uid, profile in saved.items():
        profile_rows = rows(root / "state/sessions" / profile["session_id"] / "core.sqlite3")
        credentials = next(value for owner, kind, key, value in profile_rows
                           if (owner, kind, key) == ("transport", "credentials", "session"))
        cookies = credentials["cookies"]
        serialized = json.dumps(cookies)
        cleared = not cookies and credentials["refresh_token"] is None
        if uid == "2":
            assert cleared, "logged-out credentials were retained"
        else:
            assert "MUSIC_U=fixture" + uid in serialized
            # These fixture completions return both MUSIC_U and refresh token.
            # An unpaired verification acknowledgement cannot install refresh.
            expected_refresh = "fixture-refresh"
            assert credentials["refresh_token"] == expected_refresh
        check[uid] = {"cleared": cleared, "auto_login": profile["auto_login"]}
    secrets = ["fixture-password", "wrong-password", "123456",
               hashlib.md5(b"fixture-password").hexdigest(), hashlib.md5(b"wrong-password").hexdigest()]
    databases = list((root / "state").rglob("core.sqlite3"))
    for path in databases:
        for _, _, _, value in rows(path):
            serialized = json.dumps(value)
            assert all(secret not in serialized for secret in secrets), "login input persisted"
    stages = []
    for directory, expected_count in [("first", 13), ("reopen", 3)]:
        run = root / directory
        complete = json.loads((run / "complete.json").read_text(encoding="utf-8"))
        assert complete["ok"] and complete["nativeAccount"]["nickname"] == expected["5"]
        paths = list(run.glob("auth_*.json"))
        assert len(paths) == expected_count
        for path in paths:
            raw = path.read_text(encoding="utf-8")
            data = json.loads(raw)
            assert all(secret not in raw for secret in
                       ["MUSIC_U=", "fixture-refresh", "fixture-two-factor", "DO_NOT_EXPORT"])
            assert data["networkRequests"] == 0 and data["overflow"] is False
            if not args.without_screenshots:
                assert path.with_suffix(".png").is_file()
            if data.get("expectGuest"):
                assert data["nativeAccount"]["status"] != "authenticated"
                assert data["nativeAccount"]["canSignOut"] is False
            else:
                assert data["nativeAccount"]["nickname"] == data["expectNickname"]
                assert data["nativeAccount"]["canSignOut"] is True
            stages.append(path.stem)
    before_switch = json.loads((root / "reopen/auth_reopen_accounts.json").read_text(encoding="utf-8"))
    assert not before_switch["fixtureRequests"].get("/api/w/login", 0)
    assert not before_switch["fixtureRequests"].get("/api/login", 0)
    assert len(before_switch["savedAccounts"]["value"]) == 4
    result = {"ok": True, "synthetic_only": True, "network_requests": 0,
              "profile_checks": check, "databases_checked": len(databases),
              "credential_inputs_absent": True, "auto_login_disabled_restart_guest": True,
              "explicit_saved_switch_verified": True, "stages": stages}
    (root / "independent-audit.json").write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
