#!/usr/bin/env python3
"""Capture the production Tauri auth UI against its in-process fixture service.

Run under Xvfb/DBus with GDK_BACKEND=x11. This driver never invokes business IPC
or reads real credentials; all actions are in auth_ui_probe.js via the DOM.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time

import gi

gi.require_version("Gdk", "3.0")
from gi.repository import Gdk


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path, help="new isolated proof root, or existing root with --reopen")
    parser.add_argument("--small", action="store_true")
    parser.add_argument("--reopen", action="store_true")
    parser.add_argument("--binary", type=Path, default=Path(".build/target/debug/examples/auth_ui_probe"))
    parser.add_argument("--stage-prefix", default="auth_")
    parser.add_argument("--screenshots", type=int, default=13)
    parser.add_argument("--reopen-screenshots", type=int, default=3)
    parser.add_argument("--timeout", type=int, default=180, help="bounded process deadline in seconds")
    args = parser.parse_args()
    if not 1 <= args.timeout <= 900:
        parser.error("timeout must be between 1 and 900 seconds")
    root = args.root.resolve()
    run = root / ("reopen" if args.reopen else "first")
    if run.exists() or (not args.reopen and root.exists()):
        parser.error("proof output must be new; existing evidence is never overwritten")
    if args.reopen and not (root / "first/complete.json").is_file():
        parser.error("reopen requires a completed fixture run")
    env = os.environ.copy()
    for name in ("CUMULUS_SESSION_DIR", "CUMULUS_PROOF_SMALL", "CUMULUS_PROOF_REOPEN"):
        env.pop(name, None)
    env["CUMULUS_PROOF_DIR"] = str(root)
    if args.small:
        env["CUMULUS_PROOF_SMALL"] = "1"
    if args.reopen:
        env["CUMULUS_PROOF_REOPEN"] = "1"
    root.parent.mkdir(parents=True, exist_ok=True)
    seen = set()
    started = time.monotonic()
    timed_out = False
    with (root.parent / (root.name + "-" + run.name + "-host.log")).open("x") as log:
        process = subprocess.Popen([str(args.binary.resolve())], env=env, stdout=log, stderr=log)
        try:
            while process.poll() is None:
                if time.monotonic() - started > args.timeout:
                    timed_out = True
                    break
                for path in run.glob(args.stage_prefix + "*.json"):
                    if path.stem in seen:
                        continue
                    window = Gdk.get_default_root_window()
                    if window is None:
                        continue
                    pixbuf = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                    if pixbuf is not None:
                        pixbuf.savev(str(path.with_suffix(".png")), "png", [], [])
                        seen.add(path.stem)
                        print(json.dumps({"captured": path.stem}), flush=True)
                time.sleep(0.1)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
    complete = run / "complete.json"
    expected = args.reopen_screenshots if args.reopen else args.screenshots
    ok = (not timed_out and process.returncode == 0 and complete.is_file()
          and json.loads(complete.read_text()).get("ok") is True and len(seen) == expected)
    report = {"ok": ok, "exit_code": process.returncode, "timed_out": timed_out,
              "screenshots": len(seen), "expected_screenshots": expected}
    if run.is_dir():
        (run / "capture.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report), flush=True)
    raise SystemExit(0 if ok else 1)


if __name__ == "__main__":
    main()
