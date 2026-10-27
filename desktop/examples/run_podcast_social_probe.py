"""Capture real GTK WebView milestones; no business logic or account writes."""
import json
import os
import pathlib
import subprocess
import time

import gi

gi.require_version("Gdk", "3.0")
from gi.repository import Gdk

root = pathlib.Path(os.environ["CUMULUS_PROOF_DIR"])
suffix = "-reopened" if "CUMULUS_PROOF_REOPEN" in os.environ else ""
previous = {path: path.stat().st_mtime_ns for path in root.glob("*.json")}
seen = set()
with (root.parent / (root.name + suffix + "-host.log")).open("w") as log:
    process = subprocess.Popen(
        [".build/target/debug/examples/podcast_social_ui_probe"], stdout=log, stderr=log
    )
    try:
        deadline = time.monotonic() + 400
        while process.poll() is None and time.monotonic() < deadline:
            for path in root.glob("social_*.json"):
                if path.stem in seen or path.stat().st_mtime_ns <= previous.get(path, 0):
                    continue
                if not suffix and path.stem.endswith("-reopened"):
                    continue
                window = Gdk.get_default_root_window()
                picture = Gdk.pixbuf_get_from_window(
                    window, 0, 0, window.get_width(), window.get_height()
                )
                picture.savev(str(path.with_suffix(".png")), "png", [], [])
                seen.add(path.stem)
                print(json.dumps({"captured": path.stem}), flush=True)
            time.sleep(0.15)
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
            raise RuntimeError("proof timeout")
        complete = root / ("complete" + suffix + ".json")
        failed = root / ("failed" + suffix + ".json")
        fresh_complete = complete.exists() and complete.stat().st_mtime_ns > previous.get(complete, 0)
        fresh_failure = failed.exists() and failed.stat().st_mtime_ns > previous.get(failed, 0)
        print(json.dumps({"exit": process.returncode, "screenshots": len(seen)}), flush=True)
        if process.returncode or fresh_failure or not fresh_complete:
            raise SystemExit(1)
        baseline = json.loads((root / "baseline-verified.json").read_text())
        if not (baseline["subscription_ids_and_order_restored"] and baseline["liked_ids_and_order_restored"] and baseline["resume_restored"] and baseline["pending"] == 0):
            raise SystemExit(1)
        if json.loads(complete.read_text()).get("ok") is not True:
            raise SystemExit(1)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
