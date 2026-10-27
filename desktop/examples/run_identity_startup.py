#!/usr/bin/env python3
"""Launch the production binary with explicit local identity and an authorized profile.

Read-only account bootstrap, screenshot and private SQLite inspection. No test
IPC or JavaScript is injected, and no identifier/cookie is printed.
"""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import time
import gi
gi.require_version('Gdk','3.0')
from gi.repository import Gdk


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('profile',type=Path)
    parser.add_argument('root',type=Path)
    args=parser.parse_args()
    root=args.root.resolve();profile=args.profile.resolve()
    if not (profile/'core.sqlite3').is_file():parser.error('explicit existing PC profile required')
    root.mkdir(mode=0o700)
    env=os.environ.copy()
    env.update(CUMULUS_DATA_DIR=str(root/'state'),CUMULUS_SESSION_DIR=str(profile),CUMULUS_DEVICE_IDENTITY='local')
    observed=False;captured=False;failure=None
    with (root/'host.log').open('x') as log:
        process=subprocess.Popen([str(Path('.build/target/debug/cumulus').resolve())],env=env,stdout=log,stderr=log)
        deadline=time.monotonic()+90
        try:
            while process.poll() is None and time.monotonic()<deadline:
                db_path=root/'state/core.sqlite3'
                if db_path.exists():
                    try:
                        with sqlite3.connect(db_path.as_uri()+'?mode=ro',uri=True,timeout=1) as db:
                            identity=db.execute("SELECT count(*) FROM state WHERE owner='device' AND kind='identity' AND key='client_sign'").fetchone()[0]
                            account=db.execute("SELECT count(*) FROM state WHERE kind='account' AND key='profile' AND owner!='device'").fetchone()[0]
                            writes=db.execute("SELECT count(*) FROM state WHERE kind IN ('comment-writes-v1','comment-likes-v1','playlist-operations-v1')").fetchone()[0]
                        if identity==1 and account>=1:
                            assert writes==0
                            observed=True;break
                    except sqlite3.OperationalError:
                        pass
                time.sleep(0.1)
            if not observed or process.poll() is not None:raise RuntimeError('production account bootstrap did not finish')
            time.sleep(2)
            if process.poll() is not None:raise RuntimeError('production window exited before capture')
            window=Gdk.get_default_root_window()
            pixbuf=Gdk.pixbuf_get_from_window(window,0,0,window.get_width(),window.get_height()) if window else None
            if pixbuf is None:raise RuntimeError('no actual desktop image')
            pixbuf.savev(str(root/'startup.png'),'png',[],[]);captured=True
        except Exception as error:
            failure=str(error)
        finally:
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:process.kill();process.wait()
    report=dict(ok=observed and captured and failure is None,productionBinary=True,identityEnabled=True,
                authenticatedStateObserved=observed,screenshot=captured,writeActionsTriggered=0,
                shutdown='controlled termination after read-only startup, not crash recovery proof',error=failure)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report),flush=True)
    raise SystemExit(0 if report['ok'] else 1)


if __name__=='__main__':main()
