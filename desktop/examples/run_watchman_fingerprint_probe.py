#!/usr/bin/env python3
"""Capture real WebView proof; live network/account access requires --live."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
import gi
gi.require_version('Gdk', '3.0')
from gi.repository import Gdk

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root',type=Path)
    parser.add_argument('--collection',action='store_true')
    parser.add_argument('--live',action='store_true',help='Explicit native reports and fixed authorized account reads; requires CUMULUS_SESSION_DIR')
    writes=parser.add_mutually_exclusive_group()
    writes.add_argument('--write',action='store_true',help='With --live: one private EAPI create and verified fixture cleanup')
    writes.add_argument('--recover-write',type=Path,help='With --live: read original write journal and recover its cleanup; never recreate')
    writes.add_argument('--playlist',action='store_true',help='With --live: isolated official EAPI playlist CRUD/order workflow')
    writes.add_argument('--recover-playlist',type=Path,help='With --live: reconcile and clean up an original isolated playlist workflow')
    args=parser.parse_args()
    root=args.root.resolve()
    assert not root.exists()
    os.umask(0o077)
    env=os.environ.copy();env['CUMULUS_PROOF_DIR']=str(root)
    env.pop('CUMULUS_PROOF_WRITE',None);env.pop('CUMULUS_PROOF_WRITE_RECOVER',None)
    env.pop('CUMULUS_PROOF_PLAYLIST',None);env.pop('CUMULUS_PROOF_PLAYLIST_RECOVER',None)
    if args.write or args.recover_write or args.playlist or args.recover_playlist:
        assert args.live, 'write/recovery requires --live'
    if args.write:env['CUMULUS_PROOF_WRITE']='1'
    if args.recover_write:env['CUMULUS_PROOF_WRITE_RECOVER']=str(args.recover_write.resolve())
    if args.playlist:env['CUMULUS_PROOF_PLAYLIST']='1'
    if args.recover_playlist:env['CUMULUS_PROOF_PLAYLIST_RECOVER']=str(args.recover_playlist.resolve())
    with root.parent.joinpath(root.name+'-driver.log').open('x') as log:
        assert not (args.collection and args.live)
        name='watchman_live_probe' if args.live else ('watchman_collection_probe' if args.collection else 'watchman_fingerprint_probe')
        process=subprocess.Popen([str(Path('.build/target/debug/examples',name).resolve())],env=env,stdout=log,stderr=log)
        try:
            until=time.monotonic()+(240 if args.write or args.recover_write or args.playlist or args.recover_playlist else (120 if args.live else 60))
            while not (root/'complete.json').exists():
                if process.poll() is not None:raise RuntimeError('proof exited before completion')
                if time.monotonic()>until:raise RuntimeError('proof timed out')
                time.sleep(.1)
            report=json.loads((root/'complete.json').read_text());assert report['ok']
            # The WebView has crossed two animation frames; allow the X server
            # to present its surface before taking the desktop screenshot.
            time.sleep(.4)
            window=Gdk.get_default_root_window();assert window
            pix=Gdk.pixbuf_get_from_window(window,0,0,window.get_width(),window.get_height());assert pix
            pix.savev(str(root/'complete.png'),'png',[],[])
            assert process.wait(timeout=10)==0
            print(json.dumps(report),flush=True)
        finally:
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:process.kill();process.wait()

if __name__=='__main__':main()
