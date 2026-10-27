"""Linux test orchestration only; business logic runs in Rust and the real UI.
Usage: python3 desktop/examples/run_download_probe.py NEW_PROOF_DIR PC_PROFILE [--small] [--cloud]
Requires Pillow, Xvfb, D-Bus, X11/XTest, ffprobe and the built ui_probe example.
"""
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time
from PIL import ImageGrab


def cancel_native_picker(display):
    x = ctypes.CDLL(ctypes.util.find_library('X11'))
    xt = ctypes.CDLL(ctypes.util.find_library('Xtst'))
    x.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x.XOpenDisplay.restype = ctypes.c_void_p
    x.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x.XKeysymToKeycode.restype = ctypes.c_uint
    x.XFlush.argtypes = [ctypes.c_void_p]
    x.XCloseDisplay.argtypes = [ctypes.c_void_p]
    xt.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    handle = x.XOpenDisplay(display.encode())
    if not handle:
        raise RuntimeError('cannot open isolated X display')
    try:
        key = x.XKeysymToKeycode(handle, 0xff1b)
        xt.XTestFakeKeyEvent(handle, key, 1, 0)
        xt.XTestFakeKeyEvent(handle, key, 0, 0)
        x.XFlush(handle)
    finally:
        x.XCloseDisplay(handle)


def main():
    proof = Path(sys.argv[1]).resolve()
    session = Path(sys.argv[2]).resolve()
    if not (session / 'core.sqlite3').is_file():
        raise RuntimeError('existing PC profile required')
    proof.mkdir(mode=0o700)
    readfd, writefd = os.pipe()
    xvfb = subprocess.Popen(['Xvfb', '-displayfd', str(writefd), '-screen', '0', '1280x860x24', '-nolisten', 'tcp'], pass_fds=(writefd,), stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    os.close(writefd)
    with os.fdopen(readfd) as stream:
        display = ':' + stream.readline().strip()
    app = None
    try:
        env = dict(os.environ, DISPLAY=display, GDK_BACKEND='x11', LIBGL_ALWAYS_SOFTWARE='1', CUMULUS_PROOF_DOWNLOAD='1', CUMULUS_PROOF_DIR=str(proof), CUMULUS_DATA_DIR=str(proof / 'profile'), CUMULUS_SESSION_DIR=str(session))
        if '--cloud' in sys.argv:
            env.pop('CUMULUS_PROOF_DOWNLOAD')
            env['CUMULUS_PROOF_CLOUD_DOWNLOAD'] = '1'
        if '--small' in sys.argv:
            env['CUMULUS_PROOF_SMALL'] = '1'
        with (proof / 'app.log').open('w') as log:
            app = subprocess.Popen(['dbus-run-session', '--', str(Path('.build/target/debug/examples/ui_probe').resolve())], env=env, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            seen = set()
            began = time.monotonic()
            while time.monotonic() - began < 360:
                for path in proof.glob('*.json'):
                    if path.stem in seen or path.stem in {'runner', 'media-check'}:
                        continue
                    value = json.loads(path.read_text())
                    stage = value.get('stage')
                    if not stage:
                        continue
                    if stage not in {'complete', 'failed'}:
                        ImageGrab.grab(xdisplay=display).save(proof / (stage + '.png'))
                    if stage == 'download_picker':
                        windows = subprocess.check_output(['xwininfo', '-root', '-tree'], env=env, text=True)
                        if '选择下载目录' not in windows:
                            raise RuntimeError('native directory chooser window not observed')
                        cancel_native_picker(display)
                    if stage == 'download_complete':
                        media = Path(value['filePath']).resolve()
                        if not media.is_relative_to(proof / 'media'):
                            raise RuntimeError('download left isolated media directory')
                        result = subprocess.run(['ffprobe', '-v', 'error', '-show_entries', 'format=duration,format_name:stream=codec_name,sample_rate,channels', '-of', 'json', str(media)], capture_output=True, text=True, check=True)
                        facts = json.loads(result.stdout)
                        if float(facts['format']['duration']) < 10 or not facts['streams']:
                            raise RuntimeError('download is not playable audio')
                        facts['lrc_exists'] = media.with_suffix('.lrc').is_file()
                        if '--cloud' in sys.argv:
                            subprocess.run(['ffmpeg', '-nostdin', '-v', 'error', '-i', str(media), '-f', 'null', '-'], capture_output=True, check=True)
                            facts['full_decode'] = True
                        (proof / 'media-check.json').write_text(json.dumps(facts, indent=2) + '\n')
                    seen.add(path.stem)
                    print('Observed:', stage, flush=True)
                if app.poll() is not None:
                    break
                time.sleep(.1)
            else:
                raise RuntimeError('desktop download proof timed out')
            complete = proof / 'complete.json'
            remaining = [str(p.relative_to(proof)) for folder in ['media', 'migrated'] for p in (proof / folder).rglob('*') if p.is_file()]
            result = {'process_exit': app.returncode, 'complete': complete.is_file() and json.loads(complete.read_text()).get('ok') is True, 'media_removed': not remaining, 'remaining_file_count': len(remaining), 'captured_stages': sorted(seen), 'window': [760, 540] if '--small' in sys.argv else [1120, 760]}
            (proof / 'runner.json').write_text(json.dumps(result, indent=2) + '\n')
            if not result['complete'] or app.returncode != 0 or remaining:
                raise RuntimeError('desktop download proof did not complete; inspect private reports')
            print('Real Tauri download, playback and file-management proof passed; media removed.', flush=True)
    finally:
        if app and app.poll() is None:
            os.killpg(app.pid, signal.SIGTERM)
            try:
                app.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(app.pid, signal.SIGKILL)
                app.wait()
        xvfb.terminate()
        xvfb.wait(timeout=5)


if __name__ == '__main__':
    main()
