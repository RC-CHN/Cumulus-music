"""Native startup and expiry membership reads in an isolated desktop WebView."""

import argparse
import json
import os
from pathlib import Path
import sys

from playback_power_smoke import windows_runtime
from window_manager import launch


def read(path):
    return json.loads(path.read_text(encoding='utf-8'))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
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
               CUMULUS_SEARCH_UI_PROOF='1', CUMULUS_SEARCH_V3_PROOF='1',
               CUMULUS_SEARCH_PRIVILEGE_PROOF='1', CUMULUS_MEMBER_LIFECYCLE_PROOF='1',
               CUMULUS_UX_SCRIPT=str(Path(__file__).resolve().parents[2]
                                     / 'desktop/examples/member_lifecycle_ui_probe.js'))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if windows:
        stage, _ = windows_runtime(binary, args.runtime)
        env['MPV_DLL_DIR_CUMULUS'] = str(stage)
        dirs = ('APPDATA', 'LOCALAPPDATA', 'TEMP', 'TMP')
    else:
        dirs = ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR')
        env.update(LC_ALL='en_US.UTF-8', GDK_BACKEND='x11')
    for name in dirs:
        folder = root / name
        folder.mkdir(mode=0o700)
        env[name] = str(folder)

    out = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(binary, env, log, out, args.screenshots)
    failed = out / 'failed.json'
    assert not failed.exists(), read(failed) if failed.exists() else None
    complete = read(out / 'complete.json')
    assert code == 0 and complete['ok'] and complete['stageCount'] == 3
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    login, active, expiry = (read(out / f'ux_{number:02}_{name}.json')
                             for number, name in enumerate(('login_verified', 'active', 'expired'), 1))
    assert login['membershipLifecycleFixture']['membershipReads'] == 1
    assert login['mv'].startswith('查看替代 MV') and login['vip'] is None
    assert active['cached'] == {'vinyl': True, 'plus': True}
    assert active['mv'].startswith('播放 MV') and active['vip'] == 'VIP'
    assert expiry['cachedAtExpiry'] == {'vinyl': False, 'plus': True}
    assert expiry['mv'].startswith('查看替代 MV') and expiry['vip'] is None
    fixture = complete['membershipLifecycleFixture']
    assert fixture['membershipReads'] == 2 and fixture['calls'][0] == 'account_get'
    assert [call for call in fixture['calls'] if call != 'account_get'] == [
        'membership', 'privileges_active', 'membership', 'privileges_expired']
    assert fixture['recheckAfterMs'] >= fixture['firstExpiryMs'] + 1_000
    screenshots = len(list(out.glob('ux_*.png')))
    assert screenshots == (3 if args.screenshots else 0)
    report = dict(ok=True, actualWebView=True, sealedMembershipLifecycle=True,
                  screenshots=screenshots, fixture=fixture,
                  platform='windows' if windows else 'linux',
                  realAccountWrites=0, realMusicRequests=0)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
