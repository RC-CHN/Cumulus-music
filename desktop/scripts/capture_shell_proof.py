"""Capture the real isolated shell; no business implementation lives here."""
import json
import os
from pathlib import Path
import subprocess
import time
import gi
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk

root = Path(os.environ["CUMULUS_PROOF_DIR"])
seen = set()
with (root.parent / (root.name + "-host.log")).open("w") as log:
    process = subprocess.Popen(["dbus-run-session", "--", ".build/target/debug/examples/shell_ui_probe"], stdout=log, stderr=log)
    deadline = time.monotonic() + 120
    try:
        while process.poll() is None and time.monotonic() < deadline:
            for path in root.glob("shell_*.json"):
                if path.stem in seen:
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
            raise RuntimeError("shell proof exceeded deadline")
        print(json.dumps({"exit": process.returncode, "screenshots": len(seen)}), flush=True)
        if process.returncode or not (root / "complete.json").exists():
            raise SystemExit(1)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
