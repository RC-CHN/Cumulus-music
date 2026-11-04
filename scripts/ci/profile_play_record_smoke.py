"""Production listening-rank WebView with a closed synthetic account."""
import argparse
import json
import os
from pathlib import Path

from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/profile_play_record_ui_probe.js'
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_PLAYLIST_CREATOR_PROOF='1', CUMULUS_PROFILE_PLAY_RECORD_PROOF='1',
               CUMULUS_UX_SCRIPT=str(source))
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        folder = root / 'os-profile' / key
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'

    names = ['missing_guest', 'hidden_guest', 'guest_header', 'guest_week', 'guest_play',
             'guest_all', 'guest_week_restored', 'guest_return', 'guest_revoked',
             'owner_header', 'owner_week', 'owner_retry', 'owner_all']
    def read(stage):
        return json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))

    stages = {name: read(f'ux_{index:02d}_{name}') for index, name in enumerate(names, 1)}
    complete = read('complete')
    assert complete['ok'] and complete['stageCount'] == len(stages)
    assert len(list(output.glob('ux_*.png'))) == (len(stages) if args.screenshots else 0)
    def calls(stage):
        value = complete if stage == 'complete' else stages[stage]
        return [(call['path'], call['args'], call['cached']) for call in value['profileRankCalls']]

    def detail(uid):
        return (f'/api/w/v1/user/detail/{uid}', {'all': True, 'userId': uid}, False)

    def record(uid, kind):
        return ('/api/v1/play/record', {'offset': 0, 'total': True, 'limit': 120,
                                       'uid': uid, 'type': kind}, False)

    assert calls('missing_guest') == [detail('4')]
    assert calls('hidden_guest') == [detail('4'), detail('5')]
    header = [detail('4'), detail('5'), detail('3')]
    assert calls('guest_header') == header
    weekly = header + [detail('3'), record('3', 1)]
    assert calls('guest_week') == calls('guest_play') == weekly
    overall = weekly + [detail('3'), record('3', 0)]
    assert calls('guest_all') == overall
    restored = overall + [detail('3'), record('3', 1)]
    assert calls('guest_week_restored') == calls('guest_return') == restored
    revoked = restored + [detail('3')]
    assert calls('guest_revoked') == revoked
    owner = revoked + [detail('2')]
    assert calls('owner_header') == owner
    owner_week = owner + [record('2', 1)]
    assert calls('owner_week') == owner_week
    assert calls('owner_retry') == owner_week + [record('2', 1), record('2', 0)]
    assert calls('complete') == calls('owner_retry') + [record('2', 0)]

    playback = stages['guest_play']['nativePlayer']
    assert playback['track']['id'] == '82' and playback['state'] == 'playing' and playback['position'] > .25
    assert [track['track']['id'] for track in stages['guest_play']['nativeQueue']['tracks']] == ['81', '82']
    report = {'ok': True, 'actualWebView': True, 'states': len(stages),
              'screenshots': len(stages) if args.screenshots else 0,
              'nativePlayback': 'libmpv/null', 'physicalAudioVerified': False,
              'guestRowsHidePlayCounts': True, 'guestRevocationBlockedBeforeRankRead': True,
              'realMusicRequests': 0, 'realAccountWrites': 0}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
