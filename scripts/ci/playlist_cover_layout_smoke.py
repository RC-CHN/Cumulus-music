"""Real WebView cover crop layout, drag/zoom, cancellation and upload rejection states."""
import argparse
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import threading
import time
from motion_smoke import launch


def native_picker_worker(root, env, capture, stopped, result):
    """Linux-only local inspection; the default cross-platform CI uses core seeds."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'desktop/examples'))
    seen = set()
    try:
        from run_cloud_probe import select_native_file
        from run_download_probe import cancel_native_picker
        from PIL import ImageGrab
        while not stopped.is_set():
            for path in sorted((root / 'first').glob('ux_*_picker_*.json')):
                if path in seen:
                    continue
                deadline = time.monotonic() + 12
                while not stopped.is_set() and time.monotonic() < deadline:
                    tree = subprocess.check_output(['xwininfo', '-root', '-tree'], env=env, text=True, timeout=3)
                    candidates = [line for line in tree.splitlines() if '选择歌单封面' in line]
                    mapped = False
                    for line in candidates:
                        window_id = re.search(r'0x[0-9a-fA-F]+', line).group()
                        info = subprocess.run(['xwininfo', '-id', window_id], env=env,
                                              text=True, capture_output=True, timeout=3)
                        mapped |= 'Map State: IsViewable' in info.stdout
                    if mapped:
                        break
                    time.sleep(.1)
                else:
                    raise RuntimeError('native cover chooser not observed')
                time.sleep(.2)
                if capture:
                    ImageGrab.grab(xdisplay=env['DISPLAY']).save(path.with_name('native_' + path.stem + '.png'))
                if '_cancel_' in path.stem:
                    cancel_native_picker(env['DISPLAY'])
                else:
                    name = 'invalid-cover.bmp' if '_invalid_' in path.stem else 'synthetic-cover.bmp'
                    select_native_file(env['DISPLAY'], root / name)
                seen.add(path)
            stopped.wait(.1)
        result['pickerActions'] = len(seen)
    except Exception as error:
        result['pickerError'] = str(error)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--baseline', action='store_true')
    parser.add_argument('--native-picker', action='store_true', help='local Linux GTK picker inspection')
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    assert not args.native_picker or sys.platform == 'linux', 'native picker probe requires Linux/X11'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYLIST_EDITOR_PROOF='1', CUMULUS_COVER_LAYOUT_PROOF='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/playlist_cover_layout_ui_probe.js'
    script = root / 'playlist-cover-layout.js'
    script.write_text('window.__coverLayoutBaseline=' + str(args.baseline).lower() + ';\n'
                      + 'window.__coverNativePicker=' + str(args.native_picker).lower() + ';\n'
                      + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    stopped, picker_result = threading.Event(), {}
    picker = None
    if args.native_picker:
        picker = threading.Thread(target=native_picker_worker,
                                  args=(root, env, args.screenshots, stopped, picker_result), daemon=True)
        picker.start()
    try:
        with (root / 'host.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    finally:
        stopped.set()
        if picker:
            picker.join(timeout=15)
            assert not picker.is_alive(), 'native picker worker did not stop'
    assert 'pickerError' not in picker_result, picker_result
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    expected = 60 if args.native_picker else 36
    assert complete['ok'] and complete['stageCount'] == expected
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['syntheticMetadataWrites'] == 0
    if args.native_picker:
        assert picker_result['pickerActions'] == 16 and complete['nativePickerVerified'] and complete['pickerFirstVerified'] and complete['draftPreserved']
    assert all(complete[key] for key in ['sourceImageDecodedByCore', 'cropControlsVerified', 'cancelAndFocusVerified', 'rejectionVisible', 'recoveryEntryVerified', 'readFailureBlocksSelection'])
    assert len(list((root / 'first').glob('ux_*.json'))) == expected
    if args.screenshots:
        assert len(list((root / 'first').glob('ux_*.png'))) == expected
    report = dict(complete, baseline=args.baseline, actualWebView=True, screenshots=expected if args.screenshots else 0,
                  nativePickerScreenshots=16 if args.native_picker and args.screenshots else 0,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False,
                  physicalDisplayRefreshVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: report[k] for k in ['ok', 'stageCount', 'screenshots', 'syntheticMetadataWrites']}))


if __name__ == '__main__':
    main()
