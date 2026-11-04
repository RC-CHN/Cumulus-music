"""Open the production follow-new-song route with an isolated synthetic account."""
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
    parser.add_argument('--notifications', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/follow_new_song_ui_probe.js'
    script = root/'follow.js'
    script.write_text(('window.__cumulusFollowNoticeProof=true;\n' if args.notifications else '') +
                      source.read_text(encoding='utf-8'), encoding='utf-8')
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_FOLLOW_NEW_SONG_PROOF='1',
               CUMULUS_UX_SCRIPT=str(script))
    if args.notifications:
        env['CUMULUS_FOLLOW_NOTICE_PROOF'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'followed new song process exited {status}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['latestPlayback'] and complete['realMusicRequests'] == 0
    stages = {path.stem.split('_', 2)[2]: json.loads(path.read_text(encoding='utf-8'))
              for path in output.glob('ux_*.json')}
    expected = {'initial_loading', 'first_page', 'album_expanded', 'next_error',
                'next_recovered', 'play_latest', 'return_to_followed'}
    if args.notifications:
        expected |= {'notification_cancelled', 'notification_unknown', 'notification_confirmed'}
    assert set(stages) == expected
    assert len(stages) == complete['stageCount'] == len(expected)
    assert all(not stage['overflow'] and stage['width'] == 760 for stage in stages.values())
    assert stages['next_error']['rows'] == 9 and stages['next_recovered']['rows'] == 10
    assert stages['first_page']['theme'] == 'light' and stages['next_error']['theme'] == 'dark'
    fixture = complete['followNewSongFixture']
    assert fixture['firstReads'] == 1 and fixture['playAllReads'] == 2
    assert fixture['nextCursors'] == [fixture['expectedNextCursor']] * 2
    assert fixture['notificationRequests'] == ([{'artistId': '13', 'switchStatus': 0},
                                                {'artistId': '12', 'switchStatus': 0}]
                                               if args.notifications else [])
    playback = stages['play_latest']['nativePlayer']
    assert playback['position'] > 0.2 and playback['queueLen'] == 2
    assert [row['track']['id'] for row in stages['play_latest']['nativeQueue']['tracks']] == ['1', '2']
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(stages)
    report = {
        'ok': True, 'actualWebView': True, 'states': len(stages),
        'screenshots': len(stages) if args.screenshots else 0,
        'nativePlayback': 'libmpv/null', 'physicalAudioVerified': False,
        'realMusicRequests': 0, 'remoteAccountWrites': 0,
        'syntheticNotificationWrites': len(fixture['notificationRequests']),
    }
    (root/'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
