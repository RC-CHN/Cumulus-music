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
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, mode=0o700, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    source = Path(__file__).resolve().parents[2] / 'desktop/examples/effect_peq_native_ui_probe.js'
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
    assert complete['ok'] and complete['stageCount'] == 5 and complete['sameNativePlayback']
    prepared = read('ux_01_prepared')['nativePeq']
    configured = read('ux_02_configured')['nativePeq']
    uncertain = read('ux_03_stop_uncertain')['nativePeq']
    reconciled = read('ux_04_reconciled')['nativePeq']
    released = read('ux_05_released')['nativePeq']
    assert prepared['materialGets'] == 1 and prepared['starts'] == prepared['attaches'] == 0
    assert configured['starts'] == configured['attaches'] == 1 and configured['nativeAttached']
    assert configured['processedSamples'] > 0 and configured['failedBlocks'] == 0
    assert uncertain['stops'] == uncertain['detaches'] == 1 and not uncertain['nativeAttached']
    assert reconciled == uncertain
    assert released == complete['nativePeq']
    assert released['stops'] == 2 and released['detaches'] == 1 and not released['nativeAttached']
    assert released['failedBlocks'] == 0
    writes = [call for call in released['calls'] if call['path'].endswith(('/set', '/stop'))]
    assert writes == [
        {'path': '/api/vipauth/app/interests/auth/set', 'result': 'ack'},
        {'path': '/api/vipauth/app/interests/auth/stop', 'result': 'lost'},
        {'path': '/api/vipauth/app/interests/auth/stop', 'result': 'ack'},
    ]
    assert all(call['result'] != 'unexpected' for call in released['calls'])

    # Audit the completed synthetic profile independently of the renderer and
    # Rust trace; do not export the opaque registration record.
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with sqlite3.connect(db.as_uri() + '?mode=ro', uri=True) as conn:
        rows = conn.execute(
            "SELECT owner,key,value FROM state WHERE kind='effect-authorizations-v1'"
        ).fetchall()
    assert len(rows) == 1 and rows[0][:2] == ('2', '300')
    lease = json.loads(rows[0][2])
    assert lease['phase'] == 'released' and lease['start_acknowledged'] and lease['stop_acknowledged']
    assert len(list(output.glob('ux_*.png'))) == (5 if args.screenshots else 0)
    print(json.dumps(dict(ok=True, actualWebView=True, actualNativePcm=True, nullOutput=True,
                          processedSamples=released['processedSamples'], syntheticStarts=1,
                          syntheticStops=2, durableReleased=True, realAccountWrites=0)))


if __name__ == '__main__':
    main()
