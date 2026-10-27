#!/usr/bin/env python3
"""Read-only audit of the production CAPTCHA UI with synthetic music HTTP."""
import argparse
import hashlib
import json
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("root", type=Path)
args = parser.parse_args()
run = args.root / "first"
stages = ["captcha_form", "captcha_window", "captcha_return", "captcha_window_again", "captcha_closed"]
proof = {name: json.loads((run / (name + ".json")).read_text()) for name in stages + ["complete", "capture"]}
expected = {"/api/login/qrcode/unikey": 1, "/api/w/login": 1, "/api/w/nuser/account/get": 1}
for name in stages + ["complete"]:
    p = proof[name]
    assert p["realAccountUsed"] is False and p["realMusicNetworkRequests"] == 0
    assert p["musicRequests"] == expected, (name, "unexpected replay or identity adoption")
for name in ["captcha_window", "captcha_window_again"]:
    p = proof[name]
    assert p["ipcDenied"] and p["officialWidget"] and p["puzzleImageLoaded"] and p["captchaWindows"] == 1
    urls = [r["url"] for r in p["resources"]]
    assert "https://c.dun.163.com/api/v2/getconf" in urls
    assert "https://c.dun.163.com/api/v3/get" in urls
    assert any(u.startswith("https://necaptcha.nosdn.127.net/") for u in urls)
    # The probe deliberately attempts app IPC; a localhost CSP denial is expected.
    assert all(v["directive"] == "connect-src" and v["host"] == "localhost" for v in p["violations"])
assert proof["captcha_return"]["captchaWindows"] == proof["captcha_closed"]["captchaWindows"] == 0
assert proof["complete"]["ok"] and proof["complete"]["captchaWindows"] == 0
assert proof["capture"]["ok"] and proof["capture"]["screenshots"] == 5
assert not (run / "failed.json").exists()
hashes = {}
for name in stages:
    path = run / (name + ".png")
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n" and len(data) > 10000
    hashes[str(path)] = hashlib.sha256(data).hexdigest()
result = {"ok": True, "scope": "real official widget loading/cancel/reopen and parent-flow cancellation; synthetic music HTTP only", "musicLoginSubmissions": 1, "realMusicNetworkRequests": 0, "humanCaptchaPassed": False, "screenshotHashes": hashes}
with (args.root / "independent-audit.json").open("x") as output:
    json.dump(result, output, ensure_ascii=False, indent=2)
print(json.dumps({k: v for k, v in result.items() if k != "screenshotHashes"}, ensure_ascii=False))
