"""Closed Linux WebKit proof. Run inside a disposable user/network namespace."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time


def ip(*arguments):
    return subprocess.check_output(["ip", *arguments], text=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--root", type=Path, required=True)
    args = parser.parse_args()
    uid_map = Path("/proc/self/uid_map").read_text(encoding="ascii").split()
    assert os.geteuid() == 0 and uid_map[::2] == ["0", "1"], "run inside unshare -Urn"
    interfaces = json.loads(ip("-j", "link", "show"))
    assert [item["ifname"] for item in interfaces] == ["lo"], "requires a closed namespace"
    assert json.loads(ip("-j", "route", "show", "default")) == [], "unexpected existing route"
    ip("link", "set", "lo", "up")
    try:
        run_probe(args)
    finally:
        if json.loads(ip("-j", "route", "show", "default", "dev", "lo")):
            ip("route", "del", "default", "dev", "lo")
        if "UP" not in interfaces[0]["flags"]:
            ip("link", "set", "lo", "down")


def run_probe(args):
    ip("route", "add", "default", "dev", "lo")
    routes = json.loads(ip("-j", "route", "show", "default"))
    assert len(routes) == 1 and routes[0]["dev"] == "lo" and "gateway" not in routes[0]
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    script = Path(__file__).resolve().parents[2] / "desktop/examples/message_connectivity_ui_probe.js"
    env = {key: value for key, value in os.environ.items() if not key.startswith("CUMULUS_")}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF="1",
               CUMULUS_MESSAGE_DRAWER_PROOF="1", CUMULUS_UX_SCRIPT=str(script),
               GIO_USE_NETWORK_MONITOR="netlink")
    for key in ("XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_RUNTIME_DIR"):
        profile = root / "os-profile" / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    stages = [
        ("ux_connectivity_link_down", "del"),
        ("ux_connectivity_link_up", "add"),
        ("ux_connectivity_reload_down", "del"),
        ("ux_connectivity_reload_up", "add"),
    ]
    completed = []
    output = root / "first"
    with (root / "host.log").open("x", encoding="utf-8") as log:
        process = subprocess.Popen([str(args.binary.resolve(strict=True))], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 105
            while process.poll() is None:
                if time.monotonic() > deadline:
                    raise TimeoutError("WebView did not finish")
                if len(completed) < len(stages):
                    stage, operation = stages[len(completed)]
                    if (output / f"{stage}.json").is_file():
                        ip("route", operation, "default", "dev", "lo")
                        completed.append(stage)
                time.sleep(.05)
        finally:
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
    failure = output / "failed.json"
    assert not failure.exists(), failure.read_text() if failure.exists() else ""
    assert process.returncode == 0 and len(completed) == len(stages)
    report = json.loads((output / "complete.json").read_text())
    assert report["ok"] and report["reloadCanceledProbe"] and report["fabricatedRejected"]
    assert report["realAccountWrites"] == report["realMusicRequests"] == 0
    assert report["messageRequests"]["requests"] == [["/api/pl/count", {}]] * 2
    assert not report["messageRequests"]["blockedWrite"]
    assert [(event["type"], event["navigator"]) for event in report["trustedBrowserEvents"]
            if event["trusted"]] == [("offline", False), ("online", True)] * 2
    report["namespaceInterfaces"] = ["lo"]
    report["actualWebView"] = True
    (root / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps(report, ensure_ascii=False))


if __name__ == "__main__":
    main()
