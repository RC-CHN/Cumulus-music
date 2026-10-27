#!/usr/bin/env python3
"""Drive real X11 mouse/keyboard input into the isolated Watchman WebView proof."""
import argparse
import ctypes as C
import ctypes.util
import json
import os
from pathlib import Path
import subprocess
import time
import gi
gi.require_version('Gdk','3.0')
from gi.repository import Gdk


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root',type=Path)
    args=parser.parse_args();root=args.root.resolve()
    assert not root.exists()
    os.umask(0o077)
    x=C.CDLL(ctypes.util.find_library('X11'));t=C.CDLL(ctypes.util.find_library('Xtst'))
    x.XOpenDisplay.argtypes=[C.c_char_p];x.XOpenDisplay.restype=C.c_void_p
    x.XFlush.argtypes=[C.c_void_p];x.XCloseDisplay.argtypes=[C.c_void_p]
    x.XStringToKeysym.argtypes=[C.c_char_p];x.XStringToKeysym.restype=C.c_ulong
    x.XKeysymToKeycode.argtypes=[C.c_void_p,C.c_ulong];x.XKeysymToKeycode.restype=C.c_ubyte
    t.XTestFakeMotionEvent.argtypes=[C.c_void_p,C.c_int,C.c_int,C.c_int,C.c_ulong]
    t.XTestFakeButtonEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,C.c_ulong]
    t.XTestFakeKeyEvent.argtypes=[C.c_void_p,C.c_uint,C.c_int,C.c_ulong]
    display=x.XOpenDisplay(None);assert display
    env=os.environ.copy();env['CUMULUS_PROOF_DIR']=str(root)
    log=root.parent/(root.name+'-driver.log')
    def capture(name):
        window=Gdk.get_default_root_window();assert window
        pix=Gdk.pixbuf_get_from_window(window,0,0,window.get_width(),window.get_height());assert pix
        pix.savev(str(root/name),'png',[],[])
    with log.open('x') as output:
        process=subprocess.Popen([str(Path('.build/target/debug/examples/watchman_events_probe').resolve())],env=env,stdout=output,stderr=output)
        def wait(name,seconds=60):
            until=time.monotonic()+seconds
            while time.monotonic()<until:
                if (root/name).exists():return json.loads((root/name).read_text())
                if process.poll() is not None:raise RuntimeError('probe exited before '+name)
                time.sleep(.1)
            raise RuntimeError('probe timed out at '+name)
        try:
            ready=wait('ready.json');time.sleep(.4);capture('observing.png')
            dx=round(ready['screenX']+(ready['outerWidth']-ready['width'])/2)
            dy=round(ready['screenY']+ready['outerHeight']-ready['height'])
            def click(px,py):
                t.XTestFakeMotionEvent(display,-1,dx+px,dy+py,0);x.XFlush(display);time.sleep(.12)
                t.XTestFakeButtonEvent(display,1,1,0);t.XTestFakeButtonEvent(display,1,0,0);x.XFlush(display);time.sleep(.15)
            click(180,165);click(180,245)
            for char in 'samplepassword0123':
                key=x.XKeysymToKeycode(display,x.XStringToKeysym(char.encode()));assert key
                t.XTestFakeKeyEvent(display,key,1,0);t.XTestFakeKeyEvent(display,key,0,0);x.XFlush(display);time.sleep(.035)
            click(180,335)
            complete=wait('complete.json',30);assert complete['ok'];capture('complete.png')
            assert process.wait(timeout=10)==0
            # Proof metadata contains only typed length and element identifiers.
            assert b'samplepassword0123' not in (root/'captured.json').read_bytes()
            print(json.dumps(complete),flush=True)
        finally:
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=5)
                except subprocess.TimeoutExpired:process.kill();process.wait()
            x.XCloseDisplay(display)


if __name__=='__main__':main()
