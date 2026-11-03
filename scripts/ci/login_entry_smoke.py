"""Production credential-entry WebView and native preferences, synthetic HTTP only."""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env.update(CUMULUS_PROOF_DIR=str(root/'run'), CUMULUS_PROOF_LOGIN_ENTRY='1')
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        folder = root/'os-profile'/name
        folder.mkdir(parents=True, mode=0o700)
        env[key] = str(folder)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        result = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert result == 0, result
    done = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    states = [json.loads(p.read_text(encoding='utf-8')) for p in sorted(output.glob('ux_*.json'))]
    assert done['ok'] and done['entry'] and done['stageCount'] == len(states) == 25
    by = {s['stage'].split('_', 2)[2]: s for s in states}
    for state in states:
        assert state['focusValid'] and state['focused'] and not state['overflow'] and not state['dialogScrollable']
        assert all(item['hit'] for item in state['controls'])
        assert state['realMusicRequests'] == 0
    for name in ['sms_consent_required_light', 'submit_consent_required_light', 'invalid_phone_light',
                 'multiple_lengths_small_dark', 'email_consent_required_small_dark']:
        paths = by[name]['fixture']['paths']
        assert paths.get('/api/sms/captcha/sent', 0) == 0
        assert paths.get('/api/login', 0) == paths.get('/api/login/cellphone', 0) == 0
    assert by['sms_consent_required_light']['consentRequired']
    assert by['submit_consent_required_light']['otpLength'] == 0
    assert by['email_consent_required_small_dark']['passwordLength'] > 0
    assert done['fixture']['browserDocuments'] == ['service', 'privacy', 'children', 'privacy', 'privacy', 'email_recovery']
    assert '浏览器未能打开' in by['legal_launch_failed_light']['validation']
    assert by['country_tail_keyboard_light']['countryScrollTop'] > 0
    for name in ['country_picker_light', 'country_tail_keyboard_light', 'country_picker_small_dark']:
        assert by[name]['countryRows'] <= 18 and by[name]['countryMenu']['height'] <= 315.1
    assert done['fixture']['paths']['/api/login'] == 1
    assert done['fixture']['paths']['/api/sms/captcha/sent'] == 1
    assert done['fixture']['paths'].get('/api/login/token/refresh/v1', 0) == 0
    assert len(done['savedAccounts']['value']) == 1
    assert not done['savedAccounts']['value'][0]['autoLogin']
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(states)
    auditor = Path(__file__).resolve().parents[2]/'desktop/examples/audit_login_entry_probe.py'
    subprocess.run([sys.executable, str(auditor), str(root/'run')], check=True)
    report = dict(ok=True, actualWebView=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  realMusicRequests=0, realAccountWrites=0, browserPagesOpened=0)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
