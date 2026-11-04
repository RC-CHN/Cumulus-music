"""Production profile relation UI using closed synthetic account and native IPC."""
import argparse
import json
import os
from pathlib import Path

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--frontend-dist', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYLIST_CREATOR_PROOF='1', CUMULUS_PROFILE_RELATIONS_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        folder = root / 'os-profile' / key
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    env['CUMULUS_UX_SCRIPT'] = str(Path(__file__).resolve().parents[2] /
                                  'desktop/examples/profile_relations_ui_probe.js')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'first.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'

    def read(stage):
        return json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))

    privacy = '/api/user/personal/page/info'
    followers = '/api/user/getfolloweds/'
    following = '/api/user/getfollows/3'
    mixed = '/api/user/follow/users/mixed/get/v2'
    paths = {privacy, followers, following, mixed}
    def relations(stage):
        return [(call['path'], call['args']) for call in read(stage)['profileRelationCalls']
                if call['path'] in paths]
    check = (privacy, {'userId': '3'})
    first = (following, {'offset': 0, 'limit': 20, 'getcounts': True})
    second = (following, {'offset': 20, 'limit': 20, 'getcounts': True})
    def fans(uid):
        return (followers, {'userId': uid, 'time': 0, 'offset': 0, 'limit': 20, 'getcounts': True})
    def own(scene):
        return (mixed, {'page': {'size': 19, 'cursor': '0'}, 'size': 19, 'scene': scene,
                        'sortType': 0, 'authority': False})

    assert relations('ux_01_profile') == []
    assert relations('ux_02_guest_following') == [check, check, first]
    assert relations('ux_03_retry') == [check, check, first, second]
    prefix = [check, check, first, second, second, own(0), own(1), own(2), fans('2'), check]
    assert relations('ux_06_guest_restored') == prefix
    assert relations('ux_07_denied') == prefix + [check]
    assert relations('complete') == prefix + [check] * 5 + [fans('3')]
    assert read('complete')['ok']
    assert len(list(output.glob('ux_*.png'))) == (8 if args.screenshots else 0)
    print(json.dumps({'ok': True, 'actualWebView': True, 'guestPrivacyReads': 8,
                      'ownFollowScenes': [0, 1, 2], 'realAccountWrites': 0}))


if __name__ == '__main__':
    main()
