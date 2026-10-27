#!/usr/bin/env python3
"""Start the formal binary with an explicit authorized PC session; read-only UI smoke."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import gi
gi.require_version('Gdk', '3.0')
from gi.repository import Gdk

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('profile', type=Path)
    args = parser.parse_args()
    root, profile = args.root.resolve(), args.profile.resolve()
    assert not root.exists() and (profile / 'core.sqlite3').is_file()
    os.umask(0o077)
    root.mkdir()
    env = os.environ.copy()
    env.update(CUMULUS_DATA_DIR=str(root / 'state'), CUMULUS_SESSION_DIR=str(profile), CUMULUS_DEVICE_IDENTITY='windows-local')
    with (root / 'desktop.log').open('x') as log:
        process = subprocess.Popen([str(Path('.build/target/debug/cumulus').resolve())], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 50
            database = root / 'state/core.sqlite3'
            ready = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError('formal binary exited')
                if database.is_file():
                    with sqlite3.connect(database.as_uri() + '?mode=ro', uri=True) as db:
                        rows = db.execute("SELECT kind,value FROM state WHERE kind IN ('account','pc-mconfig-v1','watchman-device-v1')").fetchall()
                    records = {kind: json.loads(value) for kind, value in rows}
                    if {'account', 'pc-mconfig-v1', 'watchman-device-v1'} <= records.keys():
                        pc, device = records['pc-mconfig-v1'], records['watchman-device-v1']
                        if pc['version'] > 0 and device['did'] and device['tid'] and device['ni']:
                            ready = dict(pcVersion=pc['version'], deviceRevision=device['revision'], accountRestored=True)
                            break
                time.sleep(.2)
            assert ready, 'formal startup did not persist account/config/device'
            time.sleep(3)
            window = Gdk.get_default_root_window()
            pix = Gdk.pixbuf_get_from_window(window, 0, 0, window.get_width(), window.get_height())
            assert pix
            pix.savev(str(root / 'desktop.png'), 'png', [], [])
            report = dict(ok=True, formalBinary=True, syntheticUiOverlay=False, **ready,
                          automatedBusinessActions=0, playbackOrAllPagesVerified=False)
            (root / 'complete.json').write_text(json.dumps(report, indent=2) + '\n')
            print(json.dumps(report), flush=True)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()

if __name__ == '__main__':
    main()
