"""Real Linux Tauri cloud UI reads/playback; explicit isolated mutation opt-in.
python3 desktop/examples/run_cloud_probe.py NEW_PROOF_DIR PC_PROFILE [--small] [--roundtrip | --resume-proof PRIOR_PROOF]
"""
import ctypes
import ctypes.util
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
import uuid
from PIL import ImageGrab
from run_download_probe import cancel_native_picker

def select_native_file(display, path):
    # This fixture path uses the standard ASCII key layout available in Xvfb.
    if any(c not in '/.abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-' for c in str(path)):
        raise RuntimeError('fixture path requires unsupported keyboard mapping')
    x=ctypes.CDLL(ctypes.util.find_library('X11'));xt=ctypes.CDLL(ctypes.util.find_library('Xtst'))
    x.XOpenDisplay.argtypes=[ctypes.c_char_p];x.XOpenDisplay.restype=ctypes.c_void_p
    x.XKeysymToKeycode.argtypes=[ctypes.c_void_p,ctypes.c_ulong];x.XKeysymToKeycode.restype=ctypes.c_uint
    x.XFlush.argtypes=[ctypes.c_void_p];x.XCloseDisplay.argtypes=[ctypes.c_void_p]
    xt.XTestFakeKeyEvent.argtypes=[ctypes.c_void_p,ctypes.c_uint,ctypes.c_int,ctypes.c_ulong]
    handle=x.XOpenDisplay(display.encode())
    if not handle:raise RuntimeError('missing isolated X display')
    def event(symbol,down):xt.XTestFakeKeyEvent(handle,x.XKeysymToKeycode(handle,symbol),int(down),0)
    try:
        event(0xffe3,True);event(ord('l'),True);event(ord('l'),False);event(0xffe3,False);x.XFlush(handle);time.sleep(.3)
        event(0xffe3,True);event(ord('a'),True);event(ord('a'),False);event(0xffe3,False)
        for char in str(path):
            if char.isupper():event(0xffe1,True)
            event(ord(char),True);event(ord(char),False)
            if char.isupper():event(0xffe1,False)
        x.XFlush(handle);time.sleep(.6)
        event(0xff0d,True);event(0xff0d,False);x.XFlush(handle);time.sleep(.3)
    finally:x.XCloseDisplay(handle)

def main():
    proof=Path(sys.argv[1]).resolve();session=Path(sys.argv[2]).resolve()
    if not (session/'core.sqlite3').is_file():raise RuntimeError('existing PC profile required')
    proof.mkdir(mode=0o700)
    fixture=None;data=proof/'profile'
    if '--resume-proof' in sys.argv:
        if '--roundtrip' in sys.argv:raise RuntimeError('select one cloud mutation mode')
        prior=Path(sys.argv[sys.argv.index('--resume-proof')+1]).resolve()
        recovery=json.loads((prior/'private-cloud-recovery.json').read_text())
        fixture=recovery['fixture'];path=Path(fixture['path']).resolve()
        if not path.is_relative_to(prior) or hashlib.sha256(path.read_bytes()).hexdigest()!=fixture['sha256'] or not recovery.get('taskId'):
            raise RuntimeError('prior isolated fixture identity mismatch')
        fixture={**fixture,'resumeTaskId':recovery['taskId'],'baseline':recovery['baseline']};data=prior/'profile'
    if '--roundtrip' in sys.argv:
        name='cumulus-cloud-proof-'+uuid.uuid4().hex
        path=proof/(name+'.flac')
        subprocess.run(['ffmpeg','-nostdin','-v','error','-n','-f','lavfi','-i',f'anoisesrc=color=pink:amplitude=0.02:duration=12:sample_rate=44100:seed={int(uuid.uuid4().hex[:7],16)}','-c:a','flac','-metadata','title='+name,'-metadata','artist=Cumulus isolated verification','-metadata','album=Desktop cloud proof',str(path)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
        fixture={'path':str(path),'name':path.name,'bytes':path.stat().st_size,'sha256':hashlib.sha256(path.read_bytes()).hexdigest()}
    readfd,writefd=os.pipe()
    xvfb=subprocess.Popen(['Xvfb','-displayfd',str(writefd),'-screen','0','1280x860x24','-nolisten','tcp'],pass_fds=(writefd,),stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    os.close(writefd)
    with os.fdopen(readfd) as stream:display=':'+stream.readline().strip()
    app=None
    try:
        env=dict(os.environ,DISPLAY=display,GDK_BACKEND='x11',LIBGL_ALWAYS_SOFTWARE='1',CUMULUS_PROOF_CLOUD='1',CUMULUS_PROOF_DIR=str(proof),CUMULUS_DATA_DIR=str(data),CUMULUS_SESSION_DIR=str(session))
        if '--small' in sys.argv:env['CUMULUS_PROOF_SMALL']='1'
        if fixture:env['CUMULUS_PROOF_CLOUD_FIXTURE']=json.dumps(fixture)
        with (proof/'app.log').open('w') as log:
            app=subprocess.Popen(['dbus-run-session','--',str(Path('.build/target/debug/examples/ui_probe').resolve())],env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
            seen=set();began=time.monotonic()
            while time.monotonic()-began<360:
                for path in proof.glob('*.json'):
                    if path.stem in seen or path.stem=='runner':continue
                    value=json.loads(path.read_text());stage=value.get('stage')
                    if not stage:continue
                    if stage!='complete':ImageGrab.grab(xdisplay=display).save(proof/(stage+'.png'))
                    if stage in {'cloud_picker','cloud_roundtrip_picker'}:
                        windows=subprocess.check_output(['xwininfo','-root','-tree'],env=env,text=True)
                        if '选择上传到音乐云盘的文件' not in windows:raise RuntimeError('native cloud file chooser not observed')
                        if stage=='cloud_picker':cancel_native_picker(display)
                        else:select_native_file(display,Path(fixture['path']))
                    seen.add(path.stem);print('Observed:',stage,flush=True)
                if app.poll() is not None:break
                time.sleep(.1)
            else:raise RuntimeError('cloud desktop proof timed out')
            complete=proof/'complete.json'
            details=json.loads(complete.read_text()) if complete.is_file() else {}
            result={'process_exit':app.returncode,'complete':details.get('ok') is True,'captured_stages':sorted(seen),'window':[760,540] if '--small' in sys.argv else [1120,760],'remote_account_mutations':details.get('remoteAccountMutations'),'roundtrip_opt_in':bool(fixture)}
            if fixture:
                result['fixture_unchanged']=hashlib.sha256(Path(fixture['path']).read_bytes()).hexdigest()==fixture['sha256']
                recovery=proof/'private-cloud-recovery.json'
                result['cleanup_complete']=recovery.is_file() and json.loads(recovery.read_text()).get('cleanupRequired') is False
                if not result['fixture_unchanged'] or not result['cleanup_complete']:result['complete']=False
            (proof/'runner.json').write_text(json.dumps(result,indent=2)+'\n')
            if not result['complete'] or app.returncode!=0:raise RuntimeError('cloud proof incomplete; inspect private reports')
            print('Real Tauri isolated cloud upload/delete proof passed; baseline restored.' if fixture else 'Real Tauri cloud list, playback and cancelled-dialog proof passed.',flush=True)
    finally:
        if app and app.poll() is None:
            os.killpg(app.pid,signal.SIGTERM)
            try:app.wait(timeout=5)
            except subprocess.TimeoutExpired:os.killpg(app.pid,signal.SIGKILL);app.wait()
        xvfb.terminate();xvfb.wait(timeout=5)
if __name__=='__main__':main()
