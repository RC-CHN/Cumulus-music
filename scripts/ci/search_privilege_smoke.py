"""Verify membership-driven search privilege patches in an actual desktop WebView."""
import argparse
import json
import os
from pathlib import Path
import sys

from playback_power_smoke import windows_runtime
from window_manager import launch


UNCHANGED = ('cold_restricted', 'changed_tuple', 'same_tuple', 'failed_privilege_read')
STATES = (*UNCHANGED, 'play_all_refreshed')


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
    if sys.platform not in {'linux', 'win32'} or windows != bool(args.runtime) or (windows and args.screenshots):
        parser.error('Windows needs --runtime; screenshots are Linux only')

    binary, root = args.binary.resolve(strict=True), args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items()
           if not key.upper().startswith(('CUMULUS_', 'WEBVIEW2_')) and key.upper() != 'MPV_DLL_DIR_CUMULUS'}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_SEARCH_UI_PROOF='1', CUMULUS_SEARCH_V3_PROOF='1', CUMULUS_SEARCH_PRIVILEGE_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2] / 'desktop/examples/search_privilege_ui_probe.js'))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if windows:
        stage, _ = windows_runtime(binary, args.runtime)
        env['MPV_DLL_DIR_CUMULUS'] = str(stage)
        dirs = ('APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP')
    else:
        dirs = ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR')
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    for key in dirs:
        folder = root / key
        folder.mkdir(mode=0o700)
        env[key] = str(folder)

    out = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        result = launch(binary, env, log, out, args.screenshots)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(out / 'complete.json')
    assert result == 0 and complete['ok'] and complete['stageCount'] == len(STATES)
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    rows = {p.stem.split('_', 2)[2]: read(p) for p in sorted(out.glob('ux_*.json'))}
    assert tuple(rows) == STATES
    first, changed = (rows[name] for name in STATES[:2])
    assert first['labels']['mv'].startswith('查看替代 MV') and not first['refreshes']
    assert changed['labels']['mv'].startswith('播放 MV')
    assert changed['labels']['vip'] == 'VIP' and changed['labels']['free']
    assert first['quality'] == ['hires', 'sq', 'jymaster', 'sky', None, None]
    assert changed['quality'] == [None, 'sky', None, 'sq', 'hires', 'jyeffect']
    assert rows['same_tuple']['quality'] == rows['failed_privilege_read']['quality'] == changed['quality']
    assert changed['refreshes'] == [{'ok': True, 'fromNativeObservation': True,
        'ids': [str(i) for i in range(601, 607)], 'mvId': '951'}]
    assert rows['same_tuple']['refreshes'] == changed['refreshes']
    assert rows['failed_privilege_read']['refreshes'] == [
        *changed['refreshes'], {'ok': False, 'fromNativeObservation': True},
    ]
    for name in UNCHANGED:
        row = rows[name]
        assert row['sameDom'] and row['scrollTop'] == first['scrollTop'] > 0
        assert row['rowKeys'] == first['rowKeys'] and row['focused'] == first['focused']
        assert row['native']['state'] == 'paused'
        for field in ('playId', 'entryId', 'queueRevision', 'position'):
            assert row['native'][field] == first['native'][field]
    played = rows['play_all_refreshed']
    assert played['playAllIds'] == [str(i) for i in range(601, 607)]
    assert played['native']['track']['id'] == '601' and played['native']['position'] > .15
    assert played['native']['playId'] != first['native']['playId']
    fixture = complete['searchPrivilegeFixture']
    assert fixture['membershipReads'] == 4 and fixture['complexReads'] == 1
    assert fixture['privilegeReads'] == 2 and fixture['privilegeFailures'] == 1
    screenshots = len(list(out.glob('ux_*.png')))
    assert screenshots == (len(STATES) if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, nativeNullPlaybackVerified=True,
                  platform='windows' if windows else 'linux', screenshots=screenshots,
                  states=STATES, fixture=fixture, realAccountWrites=0, realMusicRequests=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2, ensure_ascii=False) + '\n', encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
