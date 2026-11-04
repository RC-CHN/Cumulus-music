"""Run two independent WebView processes on a fresh synthetic PEQ profile."""
import argparse
import json
import os
from pathlib import Path
import sqlite3

from motion_smoke import launch

CATALOGUE = '/api/usertool/sound/queryAllSound'
START = '/api/vipauth/app/interests/auth/set'
STOP = '/api/vipauth/app/interests/auth/stop'


def journal(root):
    db = (root / 'state' / 'core.sqlite3').resolve(strict=True)
    with sqlite3.connect(db.as_uri() + '?mode=ro', uri=True) as conn:
        rows = conn.execute(
            "SELECT owner,key,value FROM state WHERE kind='effect-authorizations-v1'"
        ).fetchall()
    assert len(rows) == 1 and rows[0][:2] == ('2', '300')
    return json.loads(rows[0][2])


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
    sources = Path(__file__).resolve().parents[2] / 'desktop/examples'
    first_script = root / 'first-script.js'
    first_script.write_text('window.__cumulusPeqExitUncertain = true;\n' +
                            (sources / 'effect_peq_native_ui_probe.js').read_text(encoding='utf-8'),
                            encoding='utf-8')
    env.update(CUMULUS_PROOF_DIR=str(root), CUMULUS_MOTION_PROOF='1',
               CUMULUS_EFFECT_PEQ_NATIVE_PROOF='1',
               CUMULUS_AUDIO_FILTER=str(args.library.resolve(strict=True)))
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve(strict=True)) + os.pathsep + env.get('PATH', '')
    for key in ('XDG_DATA_HOME', 'XDG_CONFIG_HOME', 'XDG_CACHE_HOME', 'XDG_RUNTIME_DIR',
                'APPDATA', 'LOCALAPPDATA'):
        profile = root / 'profile' / key
        profile.mkdir(parents=True, mode=0o700)
        env[key] = str(profile)

    def run(phase, script):
        child = dict(env, CUMULUS_UX_SCRIPT=str(script))
        if phase == 'reopen':
            child['CUMULUS_PROOF_REOPEN'] = '1'
        output = root / phase
        with (root / f'{phase}.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(strict=True), child, log, output, args.screenshots)
        failed = output / 'failed.json'
        assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
        assert code == 0, f'{phase} probe exited {code}'
        complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
        assert complete['ok'] and complete['stageCount'] == 3
        assert len(list(output.glob('ux_*.png'))) == (3 if args.screenshots else 0)
        return complete

    first = run('first', first_script)
    assert first['leftStopUncertain'] and first['lateStopVerified']
    initial_trace = first['nativePeq']
    assert initial_trace['starts'] == initial_trace['stops'] == initial_trace['detaches'] == 1
    assert not initial_trace['nativeAttached']
    assert [call for call in initial_trace['calls'] if call['path'] in (START, STOP)] == [
        {'path': START, 'result': 'ack'}, {'path': STOP, 'result': 'lost'},
    ]
    before = journal(root)
    assert before['phase'] == 'stop_uncertain' and before['start_acknowledged']
    assert not before['stop_acknowledged']

    reopened = run('reopen', sources / 'effect_peq_reopen_ui_probe.js')
    assert reopened['reopenedFromLocalList'] and reopened['otherEffectBlocked']
    reopened_trace = reopened['nativePeq']
    assert reopened_trace['processId'] != initial_trace['processId']
    assert reopened_trace['starts'] == reopened_trace['attaches'] == reopened_trace['materialGets'] == 0
    assert reopened_trace['stops'] == 1 and not reopened_trace['nativeAttached']
    assert reopened_trace['calls'] == [
        {'path': CATALOGUE, 'result': 'ack'}, {'path': STOP, 'result': 'ack'},
    ]
    for label in ('01_discovered', '02_cancelled_retry'):
        captured = json.loads((root / 'reopen' / f'ux_{label}.json').read_text(encoding='utf-8'))
        assert captured['nativePeq']['calls'] == [{'path': CATALOGUE, 'result': 'ack'}]
        assert captured['nativePeq']['stops'] == 0
    after = journal(root)
    assert after['phase'] == 'released' and after['stop_acknowledged']
    assert after['operation'] == before['operation'] and after['revision'] > before['revision']
    print(json.dumps(dict(ok=True, actualWebView=True, processRestart=True, reopenedFromLocalList=True,
                          syntheticStarts=1, syntheticStops=2, durableReleased=True,
                          realAccountWrites=0)))


if __name__ == '__main__':
    main()
