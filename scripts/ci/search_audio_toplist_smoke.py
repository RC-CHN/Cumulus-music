"""Verify assigned/control hot-search popup and exact native reads in WebView."""
import argparse
import json
import os
from pathlib import Path
import sys

from motion_smoke import launch
from playback_power_smoke import windows_runtime

LABELS = ['opened', 'typed', 'reopened']
LIST = {'kind': 'list'}
ASSIGNMENT = {'kind': 'assignment'}
DETAIL = {'kind': 'detail', 'id': 'HOT_SEARCH_DJ_VOICE_BOOK#@#'}


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    windows = sys.platform == 'win32'
    if windows and (not args.runtime or args.screenshots):
        parser.error('Windows requires the staged media runtime and no GTK screenshots')
    if not windows and (sys.platform != 'linux' or args.runtime):
        parser.error('Linux uses its system libmpv')
    binary = args.binary.resolve(strict=True)
    if windows:
        runtime, _ = windows_runtime(binary, args.runtime)
    if args.screenshots:
        import gi
        gi.require_version('Gtk', '3.0')
        gi.require_version('Gdk', '3.0')
        from gi.repository import Gdk, Gtk
        assert Gtk.init_check([])[0], 'GTK could not connect to the capture display'
        assert Gdk.get_default_root_window().get_width() > 0, 'set GDK_BACKEND=x11 under Xvfb'
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    source = (Path(__file__).resolve().parents[2] / 'desktop/examples/search_audio_toplist_ui_probe.js').read_text(encoding='utf-8')
    for case in ['assigned', 'control']:
        assigned = case == 'assigned'
        directory = root / case
        directory.mkdir()
        env = {key: value for key, value in os.environ.items()
               if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_'))
               and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
        env.update(CUMULUS_PROOF_DIR=str(directory), CUMULUS_MOTION_PROOF='1',
                   CUMULUS_SEARCH_UI_PROOF='1', CUMULUS_SEARCH_AUDIO_TOPLIST_PROOF=case)
        for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                          ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                          ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'), ('TEMP', 'temp')]:
            path = directory / 'os-profile' / name
            path.mkdir(parents=True, mode=0o700)
            env[key] = str(path)
        if windows:
            env['MPV_DLL_DIR_CUMULUS'] = str(runtime)
            env['TMP'] = env['TEMP']
        if args.frontend_dist:
            env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
        script = directory / 'search.js'
        script.write_text(f'window.__toplistAssigned = {json.dumps(assigned)};\n' + source, encoding='utf-8')
        env['CUMULUS_UX_SCRIPT'] = str(script)
        output = directory / 'first'
        with (directory / 'host.log').open('x', encoding='utf-8') as log:
            status = launch(binary, env, log, output, args.screenshots)
        failure = output / 'failed.json'
        assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
        assert status == 0, case
        complete = read(output / 'complete.json')
        assert complete['ok'] and complete['assigned'] == assigned and complete['count'] == len(LABELS)
        assert complete['realMusicRequests'] == complete['accountWrites'] == 0
        initial = [LIST, ASSIGNMENT] + ([DETAIL] if assigned else [])
        reopened = initial + [LIST] + ([DETAIL] if assigned else [])
        assert complete['audioToplistCalls'] == reopened, case
        for index, label in enumerate(LABELS, 1):
            path = output / f'ux_{index:02}_{label}.json'
            row = read(path)
            assert row['audioToplistCalls'] == (reopened if label == 'reopened' else initial), (case, label)
            assert row['noHorizontalOverflow'] and row['inputFocused'], (case, label)
            if label == 'typed':
                assert not row['names'] and not row['more'] and row['suggestions'] > 0, case
            else:
                assert row['names'] == (['热搜榜', '播客榜', '摇滚榜', '听书榜'] if assigned else ['热搜榜', '摇滚榜']), case
                assert row['more'] == (['播客榜', '听书榜'] if assigned else []), case
                assert row['suggestions'] == 0, case
            if label == 'reopened':
                assert row['width'] == 760 and row['height'] == 540, case
                assert (row['scrollTop'] > 0) == row['scrollable'] and (not assigned or row['scrollable']), case
            if args.screenshots:
                assert path.with_suffix('.png').is_file(), case
    result = dict(ok=True, actualWebView=True, assignedAndControl=True, states=2 * len(LABELS),
                  screenshots=2 * len(LABELS) if args.screenshots else 0,
                  realMusicRequests=0, realAccountWrites=0, physicalDisplayVerified=False)
    (root / 'audit.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(result))


if __name__ == '__main__':
    main()
