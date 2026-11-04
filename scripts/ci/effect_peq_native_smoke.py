"""Real WebView and native PEQ lifecycle using closed synthetic API/material/PCM."""
import argparse
import json
import os
from pathlib import Path
import sqlite3

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--player-stop', action='store_true', help='stop ordinary playback before explicit PEQ cleanup')
    mode.add_argument('--resume-after-stop', action='store_true', help='resume the exact old PEQ registration')
    mode.add_argument('--pause-local', action='store_true', help='pause and resume PEQ without interrupting playback')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/effect_peq_native_ui_probe.js'
    player_stop = args.player_stop or args.resume_after_stop
    retained_registration = args.resume_after_stop or args.pause_local
    if player_stop or args.pause_local:
        script = root / 'scenario.js'
        script.write_text(('window.__cumulusPeqPlayerStop = true;\n' if player_stop else '') +
                          ('window.__cumulusPeqResume = true;\n' if args.resume_after_stop else '') +
                          ('window.__cumulusPeqPauseLocal = true;\n' if args.pause_local else '') +
                          source.read_text(encoding='utf-8'),
                          encoding='utf-8')
        source = script
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(source), CUMULUS_MOTION_PROOF='1',
               CUMULUS_EFFECT_PEQ_NATIVE_PROOF='1', CUMULUS_AUDIO_FILTER=str(args.library.resolve(strict=True)))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'probe exited {code}'

    def read(name):
        return json.loads((output / (name + '.json')).read_text(encoding='utf-8'))

    complete = read('complete')
    stages = 4 if player_stop or args.pause_local else 5
    assert complete['ok'] and complete['stageCount'] == stages
    prepared = read('ux_01_prepared')['nativePeq']
    configured = read('ux_02_configured')['nativePeq']
    assert prepared['materialGets'] == 1 and prepared['starts'] == prepared['attaches'] == 0
    assert configured['starts'] == configured['attaches'] == 1 and configured['nativeAttached']
    assert configured['processedSamples'] > 0 and configured['failedBlocks'] == 0
    if args.pause_local:
        assert complete['localPauseVerified']
        paused = read('ux_03_paused_locally')['nativePeq']
        assert paused['stops'] == 0 and paused['detaches'] == 1
        assert not paused['nativeAttached'] and paused['calls'] == configured['calls']
        final = read('ux_04_resumed_locally')['nativePeq']
    elif player_stop:
        player_stopped = read('ux_03_player_stopped')['nativePeq']
        assert player_stopped['stops'] == 0 and player_stopped['detaches'] == 1
        assert not player_stopped['nativeAttached'] and player_stopped['calls'] == configured['calls']
        if args.resume_after_stop:
            assert complete['resumedRegistrationVerified']
            final = read('ux_04_resumed_registration')['nativePeq']
        else:
            assert complete['playerStopStatusVerified']
            final = read('ux_04_player_stop_released')['nativePeq']
    else:
        assert complete['sameNativePlayback'] and complete['directManagementVerified']
        assert complete['scopedBypassVerified'] and complete['lateStopVerified']
        uncertain = read('ux_03_stop_uncertain')['nativePeq']
        reconciled = read('ux_04_reconciled')['nativePeq']
        final = read('ux_05_released')['nativePeq']
        assert uncertain['stops'] == uncertain['detaches'] == 1 and not uncertain['nativeAttached']
        assert uncertain['calls'] == configured['calls'] + [
            {'path': '/api/usertool/sound/queryAllSound', 'result': 'ack'},
            {'path': '/api/vipauth/app/interests/auth/stop', 'result': 'lost'},
        ]
        assert reconciled == uncertain
    if retained_registration:
        # A running filter keeps processing between the capture and completion.
        active = complete['nativePeq']
        assert active['processedSamples'] > final['processedSamples']
        assert {k: v for k, v in active.items() if k != 'processedSamples'} == {
            k: v for k, v in final.items() if k != 'processedSamples'
        }
        final = active
    else:
        assert final == complete['nativePeq']
    assert final['starts'] == final['materialGets'] == final['detaches'] == 1
    assert final['failedBlocks'] == 0
    writes = [call for call in final['calls'] if call['path'].endswith(('/set', '/stop'))]
    initial_write = {'path': '/api/vipauth/app/interests/auth/set', 'result': 'ack'}
    if retained_registration:
        assert final['attaches'] == 2 and final['stops'] == 0 and final['nativeAttached']
        assert final['processedSamples'] > 0 and writes == [initial_write]
    else:
        assert final['stops'] == 2 and not final['nativeAttached']
        assert writes == [
            initial_write,
            {'path': '/api/vipauth/app/interests/auth/stop', 'result': 'lost'},
            {'path': '/api/vipauth/app/interests/auth/stop', 'result': 'ack'},
        ]
    assert all(call['result'] != 'unexpected' for call in final['calls'])
    reads = [
        '/api/usertool/sound/queryAllSound',
        '/api/usertool/home/sound/info', '/api/vipauth/app/interests/auth/query',
        '/api/usertool/home/sound/info', '/api/vipauth/app/interests/auth/query',
        '/api/vipauth/app/interests/auth/query', '/api/vipauth/app/interests/auth/query',
        '/api/vipauth/app/interests/recyclequery',
    ]
    if retained_registration:
        reads += [
            '/api/usertool/home/sound/info', '/api/vipauth/app/interests/auth/query',
            '/api/vipauth/app/interests/auth/query', '/api/vipauth/app/interests/recyclequery',
        ]
    elif not player_stop:
        reads += ['/api/usertool/sound/queryAllSound']
    assert [call['path'] for call in final['calls'] if not call['path'].endswith(('/set', '/stop'))] == reads

    # Audit the completed synthetic profile independently of the renderer and
    # Rust trace; do not export the opaque registration record.
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with sqlite3.connect(db.as_uri() + '?mode=ro', uri=True) as conn:
        rows = conn.execute(
            "SELECT owner,key,value FROM state WHERE kind='effect-authorizations-v1'"
        ).fetchall()
    assert len(rows) == 1 and rows[0][:2] == ('2', '300')
    lease = json.loads(rows[0][2])
    assert lease['start_acknowledged']
    assert lease['phase'] == ('registered' if retained_registration else 'released')
    assert lease['stop_acknowledged'] is not retained_registration
    assert len(list(output.glob('ux_*.png'))) == (stages if args.screenshots else 0)
    if args.pause_local:
        checks = {'localPauseVerified': True}
    elif args.resume_after_stop:
        checks = {'resumedRegistrationVerified': True}
    elif args.player_stop:
        checks = {'playerStopStatusVerified': True}
    else:
        checks = {'scopedBypassVerified': True, 'lateStopVerified': True}
    print(json.dumps(dict(ok=True, actualWebView=True, actualNativePcm=True, nullOutput=True,
                          processedSamples=final['processedSamples'], syntheticStarts=1,
                          syntheticStops=final['stops'], durableReleased=not retained_registration,
                          realAccountWrites=0, **checks)))


if __name__ == '__main__':
    main()
