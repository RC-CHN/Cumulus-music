"""Queue clear consent and room recovery through production UI/IPC/actor.

HTTP, IM and audio devices are synthetic; this does not verify a real peer.
"""
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
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root)
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve())
    labels = ['ordinary_confirmation', 'room_changed', 'unattached_room', 'observed_room',
              'exit_confirmation', 'exit_room_changed']
    labels += [f'{stage}_{width}_{theme}' for width, theme in
               [(1120, 'light'), (760, 'light'), (760, 'dark'), (1120, 'dark')]
               for stage in ['list', 'confirm', 'cancel']]
    labels += ['clear_failed', 'failure_cancelled', 'end_pending', 'end_confirmed',
               'peer_confirmation', 'end_uncertain', 'pending_page', 'reconciled_page',
               'reopened_empty']
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        status = launch(args.binary.resolve(), env, log, root / 'first', args.screenshots)
    failed = root / 'first/failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert status == 0, f'queue-room proof exited {status}'
    complete = json.loads((root / 'first/complete.json').read_text(encoding='utf-8'))
    final = json.loads((root / 'first/host-final.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['queueRoomUi'] and complete['stageCount'] == len(labels)
    assert final['clean'] and complete['endRequests'] == 2 and complete['imClosed'] == 2
    assert complete['realMusicRequests'] == complete['realAccountWrites'] == 0
    assert complete['fixtureApi'] and complete['fixtureIm'] and complete['fixtureAudio']
    assert not complete['realPeerVerified'] and not complete['physicalAudioOutputVerified']
    for i, label in enumerate(labels, 1):
        path = root / 'first' / f'ux_{i:02}_{label}.json'
        row = json.loads(path.read_text(encoding='utf-8'))
        native, queue = row['nativePlayer'], row['nativeQueue']
        assert native['queueLen'] == len(queue['tracks'])
        assert native['queueRevision'] == queue['revision'] and not native['error']
        if row['empty']:
            assert native['state'] == 'idle' and native['playId'] is None and not queue['tracks']
        else:
            assert native['state'] == 'paused' and native['playId'] == row['playId']
            assert abs(native['position'] - 12) < .1 and len(queue['tracks']) == 3
        assert row['endRequests'] == (0 if i <= labels.index('end_pending')
                                     else 1 if i <= labels.index('end_uncertain') else 2)
        if label.startswith('confirm_') or label in ['clear_failed', 'peer_confirmation']:
            assert row['warning'] and queue['clearContext']['roomId'] == 'test-room'
        if label == 'end_pending':
            assert row['endStarted'] and row['together']['pending'][0]['action'] == 'end'
            assert row['together']['pending'][0]['phase'] == 'sending'
        if label in ['end_uncertain', 'pending_page']:
            assert len(row['together']['tracks']) == 3
            assert all(t['name'].startswith('Song') and t['durationMs'] == 120000
                       for t in row['together']['tracks'])
        if label in ['end_confirmed', 'reconciled_page', 'reopened_empty']:
            assert not row['together']['room'] and not row['together']['localRecovery']
            assert not row['together']['pending'] and native['speed'] == 1.25
            assert native['mode'] == 'sequential'
        if args.screenshots:
            assert path.with_suffix('.png').is_file()
    report = dict(ok=True, states=len(labels), actualWebView=True,
                  screenshots=len(labels) if args.screenshots else 0,
                  syntheticEndRequests=2, realMusicRequests=0, realAccountWrites=0,
                  fixtureApi=True, fixtureIm=True, fixtureAudio=True, realPeerVerified=False,
                  physicalDisplayVerified=False, physicalAudioOutputVerified=False)
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
