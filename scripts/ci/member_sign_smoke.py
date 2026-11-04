"""Verify recorded membership check-ins through production IPC in the real WebView."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/member_sign_ui_probe.js'
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_MEMBER_SIGN_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['stageCount'] == 7
    assert complete['historyPlayback'] == '2' and complete['nativeModalFocus'] and complete['noLateDetail']
    prefix = '/api/vipnewcenter/app/'
    card = {'path': prefix + 'minidesk/music/sign/pc', 'args': {'type': 1}}
    history = {'path': prefix + 'level/user/checkin/history/list', 'args': {}}
    detail = lambda record: {'path': prefix + 'level/user/checkin/history/detail',
                             'args': {'type': 2, 'recordId': record}}
    past = detail('009007199254740993')
    today = detail(82)
    assert complete['memberSignCalls'] == [card, card, history, past, history, past,
                                           history, past, history, today, card, card]
    assert len(list(output.glob('ux_*.png'))) == (7 if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'stages': 7, 'signReads': 12,
                      'historyPlayback': True, 'nativeModalFocus': True,
                      'cachedRefresh': True, 'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
