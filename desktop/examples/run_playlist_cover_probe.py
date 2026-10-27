"""Real GTK picker + production WebView. Only the Rust guard performs account IO.
NEW_ROOT PC_PROFILE [--small] [--hold]; EXISTING_ROOT PC_PROFILE --reopen [--small]
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time
from PIL import Image, ImageGrab
from run_cloud_probe import select_native_file
from run_download_probe import cancel_native_picker


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('profile', type=Path)
    parser.add_argument('--small', action='store_true')
    parser.add_argument('--hold', action='store_true')
    parser.add_argument('--reopen', action='store_true')
    args = parser.parse_args()
    root, profile = args.root.resolve(), args.profile.resolve()
    if not (profile / 'core.sqlite3').is_file():
        parser.error('existing explicit PC profile required')
    if not args.reopen:
        root.mkdir(mode=0o700)
        image = Image.new('RGB', (640, 480))
        image.putdata([(x // 4, y // 3, 220 if x >= 320 else 30) for y in range(480) for x in range(640)])
        image.save(root / 'source.png')
        Image.new('RGB', (100, 100), 'red').save(root / 'invalid.png')
    elif not json.loads((root / 'first/complete.json').read_text()).get('held'):
        parser.error('reopen requires the held selection proof')
    run = root / ('reopen' if args.reopen else 'first')
    if run.exists(): parser.error('existing output is never overwritten')
    source_hash = hashlib.sha256((root / 'source.png').read_bytes()).hexdigest()
    readfd, writefd = os.pipe()
    xvfb = subprocess.Popen(['Xvfb', '-displayfd', str(writefd), '-screen', '0', '1280x860x24', '-nolisten', 'tcp'], pass_fds=(writefd,), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    os.close(writefd)
    with os.fdopen(readfd) as stream: display = ':' + stream.readline().strip()
    app = None
    try:
        env = dict(os.environ, DISPLAY=display, GDK_BACKEND='x11', WEBKIT_DISABLE_COMPOSITING_MODE='1', LIBGL_ALWAYS_SOFTWARE='1', CUMULUS_PROOF_DIR=str(root), CUMULUS_COVER_PROFILE=str(profile))
        for name in ('CUMULUS_SESSION_DIR','CUMULUS_PROOF_SMALL','CUMULUS_PROOF_REOPEN','CUMULUS_COVER_HOLD'): env.pop(name,None)
        if args.small: env['CUMULUS_PROOF_SMALL']='1'
        if args.reopen: env['CUMULUS_PROOF_REOPEN']='1'
        if args.hold: env['CUMULUS_COVER_HOLD']='1'
        with (root / ('reopen-host.log' if args.reopen else 'first-host.log')).open('x') as log:
            app = subprocess.Popen(['dbus-run-session','--',str(Path('.build/target/debug/examples/playlist_cover_ui_probe').resolve())], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            seen = set()
            started = time.monotonic()
            while time.monotonic()-started < 300:
                for path in sorted(run.glob('cover_*.json')):
                    if path.stem in seen: continue
                    value=json.loads(path.read_text())
                    ImageGrab.grab(xdisplay=display).save(path.with_suffix('.png'))
                    if path.stem.startswith('cover_picker_'):
                        deadline=time.monotonic()+10
                        while time.monotonic()<deadline:
                            windows=subprocess.check_output(['xwininfo','-root','-tree'],env=env,text=True)
                            if '选择歌单封面' in windows: break
                            time.sleep(.1)
                        else: raise RuntimeError('native cover chooser not observed')
                        if path.stem.endswith('cancel'): cancel_native_picker(display)
                        else: select_native_file(display,root / ('invalid.png' if path.stem.endswith('invalid') else 'source.png'))
                    seen.add(path.stem)
                    print(json.dumps({'captured':path.stem}),flush=True)
                if app.poll() is not None: break
                time.sleep(.1)
            else: raise RuntimeError('cover proof process deadline')
        complete_path=run/'complete.json'
        complete=json.loads(complete_path.read_text()) if complete_path.is_file() else {}
        expected=6 if args.hold else 5 if args.reopen else 10
        report={'ok':app.returncode==0 and complete.get('ok') is True and len(seen)==expected,
                'exit':app.returncode,'stages':sorted(seen),'expected':expected,
                'sourceUnchanged':hashlib.sha256((root/'source.png').read_bytes()).hexdigest()==source_hash,
                'cleanupOk':complete.get('cleanupOk'),'held':complete.get('held',False)}
        report['ok'] &= report['sourceUnchanged'] and (report['held'] or report['cleanupOk'] is True)
        (run/'runner.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report),flush=True)
        if not report['ok']: raise RuntimeError('cover GUI proof incomplete; inspect retained evidence and --cleanup entry')
    finally:
        if app and app.poll() is None:
            os.killpg(app.pid,signal.SIGTERM)
            try: app.wait(timeout=5)
            except subprocess.TimeoutExpired: os.killpg(app.pid,signal.SIGKILL);app.wait()
        xvfb.terminate();xvfb.wait(timeout=5)

if __name__=='__main__': main()
