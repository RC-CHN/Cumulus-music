"""Capture production WebView matching interactions; no music business logic."""
import json
import os
from pathlib import Path
import subprocess
import time
import gi
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk

root = Path(os.environ["CUMULUS_PROOF_DIR"])
suffix = "-reopened" if os.environ.get("CUMULUS_PROOF_REOPEN") else ""
seen = set()
with (root.parent / (root.name + suffix + "-host.log")).open("w") as log:
    process = subprocess.Popen(["dbus-run-session", "--", ".build/target/debug/examples/local_matching_ui_probe"], stdout=log, stderr=log)
    deadline = time.monotonic() + 200
    try:
        while process.poll() is None and time.monotonic() < deadline:
            for path in root.glob("matching_*.json"):
                if (path.stem.endswith("-reopened") != bool(suffix)) or path.stem in seen:
                    continue
                window = Gdk.get_default_root_window()
                capture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                capture.savev(str(path.with_suffix(".png")), "png", [], [])
                seen.add(path.stem)
                print(json.dumps({"captured": path.stem}), flush=True)
            time.sleep(.1)
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
            raise RuntimeError("matching proof exceeded deadline")
        print(json.dumps({"exit": process.returncode, "screenshots": len(seen)}), flush=True)
        if process.returncode or not (root / ("complete" + suffix + ".json")).exists():
            raise SystemExit(1)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
