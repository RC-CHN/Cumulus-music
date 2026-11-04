"""Real WebView effect catalogue using production IPC and a synthetic API."""
import argparse
import json
import os
from pathlib import Path
import sqlite3
from contextlib import closing

from motion_smoke import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--library', type=Path, help='also exercise live headphone tuning with this native filter')
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    script = Path(__file__).resolve().parents[2] / 'desktop/examples/effect_directory_ui_probe.js'
    if args.library:
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
    stages = 12 if args.library else 7
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
        fault = json.loads((output / 'ux_11_headphones_fault.json').read_text(encoding='utf-8'))['nativePeq']
        assert footer['nativePeq']['attaches'] == 2 and footer['nativePeq']['nativeAttached']
        reapplied = json.loads((output / 'ux_12_headphones_restored.json').read_text(encoding='utf-8'))
        assert len(footer['effectDirectoryCalls']) == len(calls) - 1
        assert reapplied['nativePeq']['attaches'] == 3 and reapplied['nativePeq']['nativeAttached']
        assert enabled['attaches'] == 1 and enabled['processedSamples'] > 0 and enabled['nativeAttached']
        assert disabled['detaches'] == 1 and not disabled['nativeAttached']
        assert fault['detaches'] == fault['attaches'] == 2 and fault['injectedFaults'] == 1 and not fault['nativeAttached']
        assert fault['processedSamples'] > 0 and fault['failedBlocks'] == 0
        assert not fault['calls'] and complete['deviceTuningVerified'] and complete['headphoneFooterVerified']
    print(json.dumps({'ok': True, 'actualWebView': True, 'syntheticReadCalls': len(calls),
                      'realAccountWrites': 0, 'limitedFreeExpiryVerified': True,
                      'deviceMaterialVerified': True, 'deviceTuningVerified': bool(args.library)}))


if __name__ == '__main__':
    main()
