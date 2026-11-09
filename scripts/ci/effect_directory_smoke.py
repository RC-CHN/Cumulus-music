"""Real WebView effect catalogue using production IPC and a synthetic API."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
from contextlib import closing

from motion_smoke import launch


def verify_reopened(args, root, env, source, original_process):
    scenario = root / 'scenario-reopen.js'
    scenario.write_text('window.__cumulusDeviceReopened = true;\n' + source.read_text(encoding='utf-8'),
                        encoding='utf-8')
    env = {**env, 'CUMULUS_PROOF_REOPEN': '1', 'CUMULUS_UX_SCRIPT': str(scenario)}
    output = root / 'reopen'
    with (root / 'reopen-host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(strict=True), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'reopened headphone proof exited {code}'

    def read(name):
        return json.loads((output / f'{name}.json').read_text(encoding='utf-8'))

    before = read('ux_01_remembered_local')
    idle = read('ux_02_configured_idle')
    playing = read('ux_03_explicit_play')
    complete = read('complete')
    assert complete['ok'] and complete['rememberedAcrossProcesses'] and complete['stageCount'] == 3
    assert before['nativePeq']['processId'] != original_process
    assert not before['effectDirectoryCalls'] and before['nativePeq']['attaches'] == 0
    assert before['nativePeq']['decoderLoads'] == idle['nativePeq']['decoderLoads'] == 0
    assert before['nativePlayer']['state'] == idle['nativePlayer']['state'] == 'idle'
    assert idle['nativePeq']['attaches'] == 1 and idle['nativePeq']['processedSamples'] == 0
    assert idle['effectDirectoryCalls'] == playing['effectDirectoryCalls'] == [
        {'path': '/api/usertool/home/soundeffect/devicesound/list', 'args': {'deviceManagerId': 4100}},
    ]
    assert playing['nativePlayer']['state'] == 'playing' and playing['nativePeq']['decoderLoads'] == 1
    assert playing['nativePeq']['attaches'] == 1 and playing['nativePeq']['processedSamples'] > 0
    for view in (before, idle, playing, complete):
        assert view['nativePeq']['materialGets'] == view['nativePeq']['starts'] == view['nativePeq']['stops'] == 0
    assert len(list(output.glob('ux_*.png'))) == (3 if args.screenshots else 0)


def verify_source_bypass(root, output, complete, screenshots):
    stages = [
        ('ux_01_fallback_standard', 71, 'vivid', 'standard', False, 1, 0),
        ('ux_02_vivid_bypassed', 72, 'standard', 'vivid', True, 1, 1),
        ('ux_03_consecutive_dolby_restored', 73, 'standard', 'dolby', False, 2, 1),
        ('ux_04_sky_bypassed', 74, 'standard', 'sky', True, 2, 2),
        ('ux_05_jymaster_restored', 75, 'standard', 'jymaster', False, 3, 2),
    ]
    assert complete['ok'] and complete['stageCount'] == len(stages)
    expected_sources = []
    activation_ids = set()
    play_ids = set()
    reports = []
    effect_reads = [
        {'path': '/api/usertool/home/soundeffect/devicebrand/list', 'args': {}},
        *[{'path': '/api/usertool/home/soundeffect/devicesound/list', 'args': {'deviceManagerId': 4100}}] * 3,
    ]
    for index, (stage, song, requested, actual, bypassed, attaches, bypasses) in enumerate(stages, 1):
        expected_sources.append({'id': song, 'requested': requested, 'returned': actual})
        proof = json.loads((output / f'{stage}.json').read_text(encoding='utf-8'))
        player = proof['nativePlayer']
        native = proof['nativePeq']
        device = player['deviceParametric']
        assert player['state'] == 'playing' and player['track']['id'] == str(song)
        assert player['actualQuality'] == actual and player['position'] > .3
        assert player['parametricProcessing'] is not bypassed
        assert device['status'] == ('bypassed_for_source' if bypassed else 'configured')
        assert ('comparison' in device) is not bypassed
        assert native['decoderLoads'] == index and native['attaches'] == attaches and native['bypasses'] == bypasses
        assert native['nativeAttached'] and native['detaches'] == native['failedBlocks'] == 0
        assert native['revision'] == native['observedRevision'] and native['processedSamples'] > 0
        assert native['bypassRevision'] == (native['revision'] if bypassed else None)
        assert proof['headphoneSourceCalls'] == expected_sources
        assert proof['effectDirectoryCalls'] == effect_reads
        assert proof['effectDirectoryMaterialGets'] == 1 and native['starts'] == native['stops'] == 0
        assert not native['calls']
        activation_ids.add(device['activationId'])
        play_ids.add(player['playId'])
        reports.append({'stage': stage, 'requested': requested, 'authorized': actual,
                        'nodeRevision': native['revision'], 'observedRevision': native['observedRevision'],
                        'adapterSamples': native['processedSamples'], 'bypassed': bypassed,
                        'processingUi': player['parametricProcessing']})
    assert len(activation_ids) == 1 and len(play_ids) == len(stages)
    assert complete['headphoneSourceCalls'] == expected_sources and complete['effectDirectoryCalls'] == effect_reads
    assert len(list(output.glob('ux_*.png'))) == (len(stages) if screenshots else 0)
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with closing(sqlite3.connect(db.as_uri() + '?mode=ro', uri=True)) as store:
        assert not store.execute("SELECT 1 FROM state WHERE kind='effect-authorizations-v1'").fetchall()
    report = {'ok': True, 'actualWebView': True, 'syntheticAudio': True, 'physicalOutput': False,
              'singleActivation': True, 'realAccountWrites': 0, 'stages': reports}
    (root / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--library', type=Path, help='also exercise live headphone tuning with this native filter')
    parser.add_argument('--source-bypass', action='store_true', help='exercise effective source quality with native PEQ')
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    if args.source_bypass and not args.library:
        parser.error('--source-bypass requires --library')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    name = 'effect_source_bypass_ui_probe.js' if args.source_bypass else 'effect_directory_ui_probe.js'
    source = Path(__file__).resolve().parents[2] / 'desktop' / 'examples' / name
    script = source
    if args.source_bypass:
        env.update(CUMULUS_EFFECT_DEVICE_NATIVE_PROOF='1', CUMULUS_EFFECT_DEVICE_SOURCE_BYPASS_PROOF='1',
                   CUMULUS_AUDIO_FILTER=str(args.library.resolve(strict=True)))
    elif args.library:
        scenario = root / 'scenario.js'
        scenario.write_text('window.__cumulusDeviceNative = true;\n' + script.read_text(encoding='utf-8'),
                            encoding='utf-8')
        script = scenario
        env.update(CUMULUS_EFFECT_DEVICE_NATIVE_PROOF='1', CUMULUS_EFFECT_PEQ_RUNTIME_FAULT='1',
                   CUMULUS_AUDIO_FILTER=str(args.library.resolve(strict=True)))
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_UX_SCRIPT=str(script),
               CUMULUS_MOTION_PROOF='1', CUMULUS_EFFECT_DIRECTORY_PROOF='1')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    output = root / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output / 'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    if args.source_bypass:
        verify_source_bypass(root, output, complete, args.screenshots)
        return
    stages = 14 if args.library else 7
    assert complete['ok'] and complete['stageCount'] == stages and complete['artworkVerified'] and complete['devicesVerified']
    assert complete['limitedFreeExpiryVerified'] and complete['deviceCacheVerified']
    assert complete['deviceMaterialVerified'] and complete['effectDirectoryMaterialGets'] == 1
    catalogue = '/api/usertool/sound/queryAllSound'
    detail = '/api/usertool/home/sound/info'
    rights = '/api/vipauth/app/interests/auth/query'
    brands = '/api/usertool/home/soundeffect/devicebrand/list'
    models = '/api/usertool/home/soundeffect/devicesound/list'
    calls = complete['effectDirectoryCalls']
    applies = 2 if args.library else 0
    assert [call['path'] for call in calls] == [
        catalogue, detail, rights, catalogue, detail, rights, detail, rights, catalogue,
        brands, *[models] * (5 + applies + bool(args.library)),
    ]
    assert [call['args']['exposeType'] for call in calls if call['path'] == catalogue] == [5, 6, 7]
    assert [call['args'] for call in calls if call['path'] in (brands, models)] == [
        {}, *[{'deviceManagerId': 4100}] * (4 + applies), {'deviceManagerId': 4200},
        *([{'deviceManagerId': 4100}] if args.library else []),
    ]
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with closing(sqlite3.connect(db.as_uri() + '?mode=ro', uri=True)) as store:
        materials = store.execute("SELECT owner,value FROM state WHERE kind='effects-materials-v1'").fetchall()
        assert not store.execute("SELECT 1 FROM state WHERE kind='effect-authorizations-v1'").fetchall()
    assert len(materials) == 1 and materials[0][0] == '2'
    record = json.loads(materials[0][1])
    assert record['phase'] == 'ready' and record['handle']['sound_type'] == 2
    assert record['handle']['effect'] == 4102
    assert len(list(output.glob('ux_*.png'))) == (stages if args.screenshots else 0)
    if args.library:
        enabled = json.loads((output / 'ux_07_headphones_enabled.json').read_text(encoding='utf-8'))['nativePeq']
        disabled = json.loads((output / 'ux_08_headphones_disabled.json').read_text(encoding='utf-8'))['nativePeq']
        footer = json.loads((output / 'ux_10_headphones_footer.json').read_text(encoding='utf-8'))
        dry = json.loads((output / 'ux_10a_headphones_comparison_dry.json').read_text(encoding='utf-8'))
        wet = json.loads((output / 'ux_10b_headphones_comparison_wet.json').read_text(encoding='utf-8'))
        fault = json.loads((output / 'ux_11_headphones_fault.json').read_text(encoding='utf-8'))['nativePeq']
        assert footer['nativePeq']['attaches'] == 2 and footer['nativePeq']['nativeAttached']
        footer_player = footer['nativePlayer']
        assert footer_player['state'] == 'playing' and footer_player['parametricProcessing']
        assert footer_player['deviceParametric']['status'] == 'configured'
        equalizer = footer_player['equalizer']
        assert equalizer['status'] == 'unavailable' and not equalizer['source_bypassed']
        assert equalizer['settings']['enabled'] and equalizer['settings']['gains'] == [3] + [0] * 9
        reapplied = json.loads((output / 'ux_12_headphones_restored.json').read_text(encoding='utf-8'))
        assert len(footer['effectDirectoryCalls']) == len(calls) - 1
        assert dry['nativePeq']['attaches'] == 2 and dry['nativePeq']['detaches'] == 2 and not dry['nativePeq']['nativeAttached']
        assert wet['nativePeq']['attaches'] == 3 and wet['nativePeq']['detaches'] == 2 and wet['nativePeq']['nativeAttached']
        assert wet['nativePeq']['processedSamples'] > 0 and wet['nativePeq']['failedBlocks'] == 0
        assert all(stage['effectDirectoryCalls'] == footer['effectDirectoryCalls'] for stage in (dry, wet))
        assert all(stage['nativePeq']['decoderLoads'] == footer['nativePeq']['decoderLoads'] == 1
                   for stage in (dry, wet))
        assert all(stage['effectDirectoryMaterialGets'] == footer['effectDirectoryMaterialGets'] for stage in (dry, wet))
        assert reapplied['nativePeq']['attaches'] == 4 and reapplied['nativePeq']['nativeAttached']
        assert enabled['attaches'] == 1 and enabled['processedSamples'] > 0 and enabled['nativeAttached']
        assert disabled['detaches'] == 1 and not disabled['nativeAttached']
        assert fault['detaches'] == fault['attaches'] == 3 and fault['injectedFaults'] == 1 and not fault['nativeAttached']
        assert fault['processedSamples'] > 0 and fault['failedBlocks'] == 0
        assert not fault['calls'] and complete['deviceTuningVerified'] and complete['headphoneFooterVerified']
        assert complete['stoppedComparisonRejected']
        verify_reopened(args, root, env, source, footer['nativePeq']['processId'])
    print(json.dumps({'ok': True, 'actualWebView': True, 'syntheticReadCalls': len(calls),
                      'realAccountWrites': 0, 'limitedFreeExpiryVerified': True,
                      'deviceMaterialVerified': True, 'deviceTuningVerified': bool(args.library),
                      'headphoneComparisonVerified': bool(args.library),
                      'rememberedAcrossProcesses': bool(args.library)}))


if __name__ == '__main__':
    main()
