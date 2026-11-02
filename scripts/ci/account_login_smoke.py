"""Actual production login/account WebView, isolated credentials and synthetic EAPI only."""
import argparse
import json
import os
from pathlib import Path
from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--baseline', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    if args.baseline:
        env['CUMULUS_PROOF_BASELINE'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
                      ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
        path = root/'os-profile'/name
        path.mkdir(parents=True, mode=0o700)
        env[key] = str(path)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve())+os.pathsep+env.get('PATH', '')
    output = root/'run/first'
    with (root/'host.log').open('x', encoding='utf-8') as log:
        code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failed = output/'failed.json'
    assert not failed.exists(), failed.read_text(encoding='utf-8') if failed.exists() else ''
    assert code == 0, f'account/login process exited {code}'
    complete = json.loads((output/'complete.json').read_text(encoding='utf-8'))
    states = [json.loads(path.read_text(encoding='utf-8')) for path in sorted(output.glob('ux_*.json'))]
    assert complete['ok'] and len(states) == complete['stageCount']
    assert len(states) == (2 if args.baseline else 28)
    for state in states:
        assert not state['overflow']
        if not args.baseline:
            assert state['focused'] and state['focusValid']
        assert all(control['hit'] for control in state['controls'])
        assert state['realMusicRequests'] == 0
    if not args.baseline:
        by_name = {state['stage'].split('_',2)[2]: state for state in states}
        assert complete['accountLogin']
        assert by_name['email_inflight_small_dark']['fixture']['pending']
        assert by_name['late_login_cancelled_dark']['fixture']['completed'] == 1
        assert by_name['late_login_cancelled_dark']['nativeAccount']['status'] != 'authenticated'
        assert by_name['logout_failed_small_light']['nativeAccount']['status'] == 'authenticated'
        assert by_name['local_logout_light']['nativeAccount']['status'] != 'authenticated'
        paths = complete['fixture']['paths']
        assert paths.get('/api/login/token/refresh/v1') == 4
        assert by_name['saved_current_small_light']['sessionRenewal']['state'] in ['refreshed', 'not_due']
        assert paths.get('/api/sms/captcha/sent') == 1
        assert paths.get('/api/logout') == 1  # Local-only fallback sends no second request.
        assert any(account['current'] and not account['autoLogin'] and account['nickname'] == '短信验证账号'
                   for account in complete['savedAccounts']['value'])
        assert paths.get('/api/w/login/cellphone') == 2
        assert paths.get('/api/w/login') == 3  # Wrong, cancelled inflight, explicit successful retry.
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(states)
    report = dict(ok=True, baseline=args.baseline, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebView=True, realMusicRequests=0, realAccountWrites=0)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
