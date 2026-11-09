"""Playlist collection layout, create-and-collect, read failures and reconciliation via production IPC and an isolated in-memory backend."""
import argparse
import json
import os
from pathlib import Path
from motion_smoke import launch, set_webview2_motion_preference


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--stall-frames', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYLIST_COLLECT_PROOF='1')
    if os.name == 'nt':
        # This proof measures the real 8px entry, which reduced motion skips.
        # motion_smoke.py separately tests the native reduced preference.
        set_webview2_motion_preference(env, reduced=False)
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
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/playlist_collect_ui_probe.js'
    script = root / 'playlist-collect.js'
    setup = 'window.__playlistStallFrames = true;\n' if args.stall_frames else ''
    script.write_text(setup + source.read_text(encoding='utf-8'), encoding='utf-8')
    env['CUMULUS_UX_SCRIPT'] = str(script)
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'
    verify(root, args.screenshots, args.stall_frames)

def verify(root, capture, stall_frames=False):
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    assert (complete['blockedFrames'] > 0) == stall_frames
    early = complete['openingFrameStall']
    assert (early is not None) == stall_frames
    if early is not None:
        assert early['layoutWidth'] == 442 and round(early['width']) != 442
        assert 'modal-enter-from' in early['classes']
    initial = json.loads((root / 'first/ux_01_list_1120_light.json').read_text(encoding='utf-8'))
    assert initial['openingFrameStall'] == early
    stable = initial['geometry']
    assert round(stable['width']) == stable['layoutWidth'] == 442
    assert stable['opacity'] == '1' and stable['transform'] == 'none'
    creation = json.loads((root / 'first/ux_04_create_1120_light.json').read_text(encoding='utf-8'))
    assert initial['motionPolicy'] == creation['motionPolicy'] == {'app': 'full', 'nativeReduced': False}
    assert creation['invalidNameConfirmed'] is True
    entrance = creation['firstCreateEntrance']
    assert entrance['shift'] == 8 and entrance['form']['transform'] != 'none'
    assert entrance['dialog']['scrollWidth'] == entrance['dialog']['clientWidth'] == 442
    assert entrance['dialog']['scrollLeft'] == 0
    assert complete['ok'] and complete['collectionUi'] and complete['creationRetryVerified'] and complete['uncertainReadbackVerified']
    assert complete['sortedPaginationVerified']
    pages = complete['syntheticCollection']['sortedPages']
    assert pages[:3] == [['default', 0], ['recently', 0], ['default', 1]]
    assert ['default', 25] in pages
    assert complete['stageCount'] == 29 and complete['maxRows'] <= 23
    assert complete['syntheticCollection']['creates'] == 1 and complete['syntheticCollection']['writes'] == 4
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert len(list((root / 'first').glob('ux_*.json'))) == complete['stageCount']
    if capture:
        assert len(list((root / 'first').glob('ux_*.png'))) == complete['stageCount']
    result = dict(complete, actualWebView=True,
                  screenshots=complete['stageCount'] if capture else 0,
                  physicalDisplayVerified=False, windowsVisualVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps({k: result[k] for k in ['ok', 'stageCount', 'screenshots', 'maxRows']}))


if __name__ == '__main__':
    main()
