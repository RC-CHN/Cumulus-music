"""Real desktop cold account restoration and legacy renewal with a sealed music backend."""
import argparse
import json
import os
from pathlib import Path
import sqlite3

from window_manager import launch


STAGE = 'legacy_renewal_checked'


def verify(case, run, screenshots):
    raw = (run / 'first' / (STAGE + '.json')).read_text(encoding='utf-8')
    report = json.loads(raw)
    assert report['ok'] and report['case'] == case, report
    assert report['realMusicRequests'] == 0
    assert report['dom'] == {'ok': True, 'renewalState': 'missing_credentials'}
    assert report['nativeAccountStatus'] == 'authenticated'
    trace = report['trace']
    assert trace['jskCount'] == trace['unexpectedAuthCount'] == 0
    assert len(trace['assignmentMs']) == 1
    first = trace['identityMs'][0]
    assert first <= trace['assignmentMs'][0]
    assert report['checkedMs'] - first >= 4000
    with sqlite3.connect((run / 'profile/core.sqlite3').as_uri() + '?mode=ro', uri=True) as db:
        journals = list(db.execute("SELECT owner,key,value FROM state WHERE kind='login_refresh'"))
    if case == 'legacy':
        assert len(trace['identityMs']) == 2 and len(trace['legacyMs']) == 1
        dispatched = trace['legacyMs'][0]
        assert dispatched - first >= 3000
        assert trace['assignmentMs'][0] <= dispatched <= trace['identityMs'][1] <= report['checkedMs']
        assert trace['legacyWireValid'] and report['legacySuccessRecorded']
        [(owner, key, value)] = journals
        assert (owner, key) == ('42', 'legacy-periodic')
        entry = json.loads(value)
        assert entry['pending'] is None and entry['last_success'] > 0
    else:
        assert len(trace['identityMs']) == 1 and not trace['legacyMs']
        assert not report['legacySuccessRecorded'] and not journals
    for secret in ['MUSIC_U', 'legacy-renewal-fixture', 'refreshToken', 'deviceId', '_sign']:
        assert secret not in raw
    assert (run / 'first' / (STAGE + '.png')).is_file() == screenshots
    return {
        'case': case,
        'observationMs': report['checkedMs'] - first,
        'legacyRequests': len(trace['legacyMs']),
        'legacyDelayMs': trace['legacyMs'][0] - first if trace['legacyMs'] else None,
        'identityReads': len(trace['identityMs']),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path, help='isolated Windows runtime dependency directory')
    parser.add_argument('--screenshots', action='store_true', help='optional Linux GTK capture')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    base = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    if args.runtime:
        base['PATH'] = str(args.runtime.resolve()) + os.pathsep + base.get('PATH', '')
    cases = []
    for case in ['legacy', 'jsk']:
        directory = root / case
        run = directory / 'run'
        env = dict(base, CUMULUS_PROOF_DIR=str(run), CUMULUS_PROOF_COHORT=case)
        for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                          ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                          ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'),
                          ('WEBVIEW2_USER_DATA_FOLDER', 'webview')]:
            path = directory / 'os-profile' / name
            path.mkdir(parents=True, mode=0o700)
            env[key] = str(path)
        with (directory / 'host.log').open('x', encoding='utf-8') as log:
            code = launch(args.binary.resolve(), env, log, run / 'first', args.screenshots,
                          capture_glob=STAGE + '.json')
        # Inspect the structured failure evidence before the exit code.
        cases.append(verify(case, run, args.screenshots))
        assert code == 0, f'{case} desktop process exited {code}'
    report = {'ok': True, 'actualWebView': True, 'syntheticOnly': True,
              'realMusicRequests': 0, 'screenshots': 2 if args.screenshots else 0, 'cases': cases}
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
