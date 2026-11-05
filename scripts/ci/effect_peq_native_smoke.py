"""Real WebView and native PEQ lifecycle using closed synthetic API/material/PCM."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
import sys

from motion_smoke import launch


def durable_lease(root):
    # Audit the synthetic profile separately from the renderer/native trace.
    # Never export the opaque registration record.
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with sqlite3.connect(db.as_uri() + '?mode=ro', uri=True) as conn:
        rows = conn.execute(
            "SELECT owner,key,value FROM state WHERE kind='effect-authorizations-v1'"
        ).fetchall()
    assert len(rows) == 1 and rows[0][:2] == ('2', '300')
    return json.loads(rows[0][2])


def verify_source_bypass(read, complete, prepared, configured, root, output, screenshots):
    assert complete['ordinarySourceBypassVerified'] and complete['stageCount'] == 5
    if sys.platform.startswith('linux'):
        locale = read('locale')
        assert locale['numeric'] == 'C'
        assert locale['messages'] != 'C' and locale['characters'] != 'C'
    stages = (
        ('02_configured', '1', 'vivid', 'standard', 1, 0, 0),
        ('03_vivid_bypassed', '2', 'standard', 'vivid', 2, 1, 0),
        ('04_consecutive_dolby_restored', '3', 'standard', 'dolby', 3, 1, 1),
        ('05_standard_retained', '4', 'standard', 'standard', 3, 1, 1),
    )
    assert prepared['decoderLoads'] == 1
    expected_calls = [
        '/api/usertool/sound/queryAllSound',
        '/api/usertool/home/sound/info', '/api/vipauth/app/interests/auth/query',
        '/api/usertool/home/sound/info', '/api/vipauth/app/interests/auth/query',
        '/api/vipauth/app/interests/auth/query', '/api/vipauth/app/interests/auth/set',
        '/api/vipauth/app/interests/auth/query', '/api/vipauth/app/interests/recyclequery',
    ]
    assert configured['calls'] == [{'path': path, 'result': 'ack'} for path in expected_calls]
    expected_sources = []
    previous_instances, previous_samples = 0, 0
    play_ids = set()
    for index, (stage, song, requested, actual, revision, bypasses, restores) in enumerate(stages, 1):
        expected_sources.append({'id': int(song), 'requested': requested, 'actual': actual})
        capture = read('ux_' + stage)
        player, native = capture['nativePlayer'], capture['nativePeq']
        dry = stage == '03_vivid_bypassed'
        assert player['track']['id'] == song and player['actualQuality'] == actual
        assert player['state'] == 'playing' and player['position'] > .35
        assert player['boundParametric'] == ('bypassed_for_source' if dry else 'configured')
        assert player['parametricProcessing'] is not dry
        assert player['playId'] and player['playId'] not in play_ids
        play_ids.add(player['playId'])
        assert native['nativeAttached'] and native['attaches'] == native['starts'] == native['materialGets'] == 1
        assert native['detaches'] == native['stops'] == native['failedBlocks'] == native['injectedFaults'] == 0
        assert native['bypasses'] == bypasses and native['restores'] == restores
        assert native['revision'] == native['observedRevision'] == revision
        assert native['bypassRevision'] == (revision if dry else None)
        assert native['decoderLoads'] == index and native['sourceCalls'] == expected_sources
        assert native['calls'] == configured['calls']
        assert native['instancesCreated'] >= previous_instances and native['instancesCreated'] > 0
        assert native['processedSamples'] >= previous_samples
        if not dry:
            assert native['processedSamples'] > previous_samples
        previous_samples, previous_instances = native['processedSamples'], native['instancesCreated']

    final = complete['nativePeq']
    assert final['revision'] == final['observedRevision'] == 3
    assert final['sourceCalls'] == expected_sources and final['calls'] == configured['calls']
    assert final['attaches'] == 1 and final['detaches'] == final['stops'] == 0
    lease = durable_lease(root)
    assert lease['phase'] == 'registered' and lease['start_acknowledged'] and not lease['stop_acknowledged']
    assert len(list(output.glob('ux_*.png'))) == (5 if screenshots else 0)
    print(json.dumps(dict(ok=True, actualWebView=True, actualNativePcm=True, nullOutput=True,
                          ordinarySourceBypassVerified=True, observedRevisions=[1, 2, 3, 3],
                          nativeAttaches=1, sourceLoads=4, syntheticStarts=1,
                          syntheticStops=0, durableReleased=False, realAccountWrites=0)))


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
    mode.add_argument('--runtime-fault', action='store_true', help='inject an optional PEQ failure into native polling')
    mode.add_argument('--source-bypass', action='store_true', help='drive registered PEQ across four authorized sources')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/effect_peq_native_ui_probe.js'
    player_stop = args.player_stop or args.resume_after_stop
    local_recovery = args.pause_local or args.runtime_fault
    retained_registration = args.resume_after_stop or local_recovery
    if player_stop or local_recovery or args.source_bypass:
        script = root / 'scenario.js'
        script.write_text(('window.__cumulusPeqPlayerStop = true;\n' if player_stop else '') +
                          ('window.__cumulusPeqResume = true;\n' if args.resume_after_stop else '') +
                          ('window.__cumulusPeqPauseLocal = true;\n' if args.pause_local else '') +
                          ('window.__cumulusPeqRuntimeFault = true;\n' if args.runtime_fault else '') +
                          ('window.__cumulusPeqSourceBypass = true;\n' if args.source_bypass else '') +
                          source.read_text(encoding='utf-8'),
                          encoding='utf-8')
        source = script
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(source), CUMULUS_MOTION_PROOF='1',
               CUMULUS_EFFECT_PEQ_NATIVE_PROOF='1', CUMULUS_AUDIO_FILTER=str(args.library.resolve(strict=True)))
    if args.runtime_fault:
        env['CUMULUS_EFFECT_PEQ_RUNTIME_FAULT'] = '1'
    if args.source_bypass:
        env['CUMULUS_EFFECT_PEQ_SOURCE_BYPASS_PROOF'] = '1'
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
    stages = 4 if player_stop or local_recovery else 5
    assert complete['ok'] and complete['stageCount'] == stages
    prepared = read('ux_01_prepared')['nativePeq']
    configured = read('ux_02_configured')['nativePeq']
    assert prepared['materialGets'] == 1 and prepared['starts'] == prepared['attaches'] == 0
    assert configured['starts'] == configured['attaches'] == 1 and configured['nativeAttached']
    assert configured['processedSamples'] > 0 and configured['failedBlocks'] == 0
    if args.source_bypass:
        verify_source_bypass(read, complete, prepared, configured, root, output, args.screenshots)
        return
    if local_recovery:
        flag = 'runtimeFaultVerified' if args.runtime_fault else 'localPauseVerified'
        assert complete[flag]
        paused = read('ux_03_runtime_fault' if args.runtime_fault else 'ux_03_paused_locally')['nativePeq']
        assert paused['stops'] == 0 and paused['detaches'] == 1
        assert not paused['nativeAttached'] and paused['calls'] == configured['calls']
        assert configured['injectedFaults'] == 0 and paused['injectedFaults'] == int(args.runtime_fault)
        final = read('ux_04_restored_after_fault' if args.runtime_fault else 'ux_04_resumed_locally')['nativePeq']
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
    assert final['injectedFaults'] == int(args.runtime_fault)
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

    lease = durable_lease(root)
    assert lease['start_acknowledged']
    assert lease['phase'] == ('registered' if retained_registration else 'released')
    assert lease['stop_acknowledged'] is not retained_registration
    assert len(list(output.glob('ux_*.png'))) == (stages if args.screenshots else 0)
    if args.runtime_fault:
        checks = {'runtimeFaultVerified': True, 'injectedFaults': 1}
    elif args.pause_local:
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
