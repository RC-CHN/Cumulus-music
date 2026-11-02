"""Actual JSK login UI/IPC lifecycle and independent persisted-credential audit.

The standalone native example owns a synthetic EAPI backend. No live login,
SMS/email send, token rotation or account modification can reach the service.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_PROOF_JSK_AUTH='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        directory = root/'os-profile'/name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    for phase in ['first', 'reopen']:
        if phase == 'reopen':
            env['CUMULUS_PROOF_REOPEN'] = '1'
        with (root/f'{phase}.log').open('x', encoding='utf-8') as log:
            result = subprocess.run([str(args.binary.resolve())], env=env, stdout=log, stderr=log, timeout=180)
        failure = root/'run'/phase/'failed.json'
        assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
        assert result.returncode == 0, f'JSK auth process exited {result.returncode}'
    complete = json.loads((root/'run/first/complete.json').read_text(encoding='utf-8'))
    assert complete['ok'] and complete['jskProtocolEnabled']
    paths = complete['fixtureRequests']
    assert paths.get('/api/login') == 5 and paths.get('/api/login/cellphone') == 1
    assert paths.get('/api/w/login', 0) == paths.get('/api/w/login/cellphone', 0) == 0
    assert paths.get('/api/user/2fa/begin') == paths.get('/api/user/2fa/list') == 1
    assert paths.get('/api/user/2fa/securityEmail/send') == paths.get('/api/sms/captcha/sent') == 1
    audit = Path(__file__).resolve().parents[2]/'desktop/examples/audit_auth_probe.py'
    with (root/'audit.log').open('x', encoding='utf-8') as log:
        result = subprocess.run([sys.executable, '-X', 'warn_default_encoding', '-W', 'error::EncodingWarning',
                                 str(audit), str(root/'run'), '--without-screenshots'],
                                stdout=log, stderr=log, timeout=30)
    assert result.returncode == 0, 'independent auth SQLite audit failed'
    evidence = json.loads((root/'run/independent-audit.json').read_text(encoding='utf-8'))
    assert evidence['ok'] and len(evidence['stages']) == 16
    report = dict(ok=True, actualWebView=True, states=16, jskPrimaryPathsVerified=True,
                  databasesChecked=evidence['databases_checked'], realMusicRequests=0,
                  realAccountWrites=0, screenshots=0)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
