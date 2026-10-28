"""Capture history/settings/catalog UI; select the native probe via environment.

On Linux, explicitly use GDK_BACKEND=x11 for Xvfb so both the app and screenshot
connection use the same display rather than an inherited Wayland session.
"""
import json
import os
from pathlib import Path
import subprocess
import time
import gi
gi.require_version('Gdk', '3.0')
from gi.repository import Gdk
root = Path(os.environ['CUMULUS_PROOF_DIR'])
binary = os.environ['CUMULUS_PROOF_BINARY']
seen = set()
with root.with_name(root.name + '-host.log').open('w') as log:
    process = subprocess.Popen(['dbus-run-session', '--', binary], stdout=log, stderr=log)
    deadline = time.monotonic() + 240
    try:
        while process.poll() is None and time.monotonic() < deadline:
            for path in (root / 'first').glob('ux_*.json'):
                if path.stem in seen:
                    continue
                window = Gdk.get_default_root_window()
                capture = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
                if capture is None:
                    raise RuntimeError('screenshot display unavailable; use GDK_BACKEND=x11 with Xvfb')
                capture.savev(str(path.with_suffix('.png')), 'png', [], [])
                seen.add(path.stem)
                print(json.dumps({'captured': path.stem}), flush=True)
            time.sleep(.1)
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
            raise RuntimeError('WebView proof timed out')
        print(json.dumps({'exit': process.returncode, 'screenshots': len(seen)}), flush=True)
        if process.returncode or not (root / 'first' / 'complete.json').exists():
            raise SystemExit(1)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
