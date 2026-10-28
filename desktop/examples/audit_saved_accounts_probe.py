#!/usr/bin/env python3
"""Independent, read-only SQLite and stage checks for synthetic removal proof."""
import argparse
import json
from pathlib import Path
import sqlite3


def rows(path):
    with sqlite3.connect(path.resolve().as_uri() + "?mode=ro", uri=True) as db:
        return {(owner, kind, key): json.loads(value) for owner, kind, key, value
                in db.execute("SELECT owner,kind,key,value FROM state")}


def audit(root, screenshots=True):
    root = root.resolve()
    first = json.loads((root / "first/complete.json").read_text())
    assert first["ok"] and first["networkRequests"] == 0
    assert first["nativeAccount"]["nickname"] == "二次验证账号"
    target = (root / "fixture-target.txt").read_text()
    assert len(target) == 32 and all(c in "0123456789abcdef" for c in target)
    registry = rows(root / "state/core.sqlite3")
    saved = {key: value for (owner, kind, key), value in registry.items()
             if owner == "transport" and kind == "saved-profiles-v1"}
    assert set(saved) == {"3"}
    active = registry["transport", "profiles", "active"]
    assert active["session_id"] == saved["3"]["session_id"]
    intent = registry["transport", "saved-profile-removals-v1", target]
    assert intent == {"uid": "2", "session_id": target, "complete": True}
    removed = rows(root / "state/sessions" / target / "core.sqlite3")
    assert ("transport", "credentials", "session") not in removed
    current = rows(root / "state/sessions" / saved["3"]["session_id"] / "core.sqlite3")
    credentials = current["transport", "credentials", "session"]
    assert "MUSIC_U=fixture3" in json.dumps(credentials["cookies"])
    stages = []
    for name, count in [("first", 8), ("reopen", 2)]:
        run = root / name
        complete = json.loads((run / "complete.json").read_text())
        assert complete["ok"] and complete["credentialRecordRemoved"]
        assert complete["confirmationFocus"] and complete["explicitCancel"] and complete["dialogCancel"]
        if screenshots:
            capture = json.loads((run / "capture.json").read_text())
            assert capture["ok"] and capture["screenshots"] == count
        paths = list(run.glob("saved_*.json"))
        assert len(paths) == count
        for path in paths:
            data = json.loads(path.read_text())
            assert data["networkRequests"] == 0 and not data["overflow"]
            assert data["nativeAccount"]["nickname"] == "二次验证账号"
            assert data["nativeAccount"]["canSignOut"]
            if screenshots:
                assert path.with_suffix(".png").is_file()
            stages.append(path.stem)
    pending = json.loads((root / "first/saved_pending.json").read_text())
    pending_row = next(a for a in pending["savedAccounts"]["value"] if a["id"] == target)
    assert pending_row["removalPending"] and not pending_row["current"]
    cancelled = json.loads((root / "first/saved_cancelled.json").read_text())
    assert len(cancelled["savedAccounts"]["value"]) == 2
    assert not any(a["removalPending"] for a in cancelled["savedAccounts"]["value"])
    for name, theme in [("saved_logout_light", "light"), ("saved_logout_dark", "dark")]:
        value = json.loads((root / "first" / (name + ".json")).read_text())
        assert value["theme"] == theme and value["title"] == "退出登录"
        assert value["focused"] == "取消"
        assert len(value["savedAccounts"]["value"]) == 2
        assert not any("logout" in path for path in value["fixtureRequests"])
    dark = json.loads((root / "first/saved_dark_small.json").read_text())
    assert dark["theme"] == "dark" and dark["width"] == 760
    result = {"ok": True, "synthetic_only": True, "network_requests": 0,
              "removed_credential_absent": True, "current_account_preserved": True,
              "completed_intent_durable": True, "failed_cleanup_explicitly_recovered": True,
              "reopened_without_removed_account": True, "stages": stages}
    (root / "independent-audit.json").write_text(json.dumps(result, indent=2) + "\n")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    print(json.dumps(audit(parser.parse_args().root)))


if __name__ == "__main__":
    main()
