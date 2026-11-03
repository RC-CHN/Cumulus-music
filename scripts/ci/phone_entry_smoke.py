"""Production WebView phone number/code/signup transitions, synthetic HTTP only."""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path
from window_manager import launch


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--binary', type=Path, required=True)
    p.add_argument('--root', type=Path, required=True)
    p.add_argument('--runtime', type=Path)
    p.add_argument('--legacy', action='store_true')
    p.add_argument('--screenshots', action='store_true')
    args = p.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {k: v for k, v in os.environ.items() if not k.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    if args.legacy:
        env['CUMULUS_PROOF_LEGACY_PHONE_ENTRY'] = '1'
    for key, name in [('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'),
                      ('XDG_CACHE_HOME', 'cache'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local')]:
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
    assert done['ok'] and done['phoneEntry']
    states = [json.loads(path.read_text(encoding='utf-8')) for path in sorted(output.glob('ux_*.json'))]
    expected = '''baseline_account_light number_light consent_required_light check_failed_light check_failed_small_dark check_inflight_dark check_cancelled_dark code_light code_small_dark code_bad_dark nickname_small_dark nickname_light nickname_short_light nickname_long_light nickname_invalid_light nickname_taken_dark nickname_check_failed_dark nickname_hint_failed_dark nickname_refresh_dark nickname_cleared_dark registration_rejected_dark registration_rate_limited_dark registration_sms_invalid_dark nickname_no_default_light nickname_cancelled_light nickname_closed_light registered_default_light existing_code_light existing_login_light registered_named_light registration_partial_dark registration_reconciled_dark registration_lost_light registration_retry_blocked_light registration_fresh_login_light register_inflight_dark register_closed_dark registration_held_fresh_login_dark send_failed_light send_unknown_small_dark nickname_hint_inflight_dark nickname_hint_cancelled_dark'''.split()
    by = {state['stage'].split('_', 2)[2]: state for state in states}
    assert list(by) == expected and done['stageCount'] == len(states) == len(expected)
    for state in states:
        assert state['focusValid'] and state['focused'] and not state['overflow']
        assert all(control['hit'] for control in state['controls'])
        assert state['realMusicRequests'] == 0
        geometry = state['nicknameGeometry']
        if geometry:
            for key, expected_value in dict(fieldWidth=300, fieldHeight=36, inputHeight=36,
                                            feedbackHeight=16, primaryWidth=300, primaryHeight=40,
                                            primaryMarginTop=50, randomDividerWidth=1,
                                            randomDividerHeight=16).items():
                assert abs(geometry[key] - expected_value) < .25, (state['stage'], key, geometry)
            assert geometry['randomVisible']
    paths = lambda name: by[name]['fixture']['paths']
    register = '/api/w/register/cellphone'
    verify = '/api/sms/captcha/verify'
    assert paths('consent_required_light').get('/api/cellphone/existence/check', 0) == 0
    assert paths('check_cancelled_dark').get('/api/middle/captcha/sent/v1', 0) == 0
    assert paths('nickname_taken_dark').get(register, 0) == 0
    assert paths('nickname_invalid_light').get('/api/nickname/duplicated', 0) == 0
    assert paths('nickname_invalid_light').get(register, 0) == 0
    assert by['nickname_taken_dark']['nicknameLength'] == len('重复昵称')
    assert by['nickname_check_failed_dark']['nicknameLength'] == len('查重失败昵称')
    assert by['nickname_hint_failed_dark']['nicknameLength'] == len('推荐失败保留昵称')
    assert by['nickname_cleared_dark']['nicknameLength'] == 0
    assert paths('registration_rate_limited_dark')[register] == 2
    assert paths('nickname_closed_light')[register] == 3
    assert paths('existing_code_light').get(verify) == paths('existing_login_light').get(verify)
    assert paths('registration_partial_dark')[register] == paths('registration_reconciled_dark')[register]
    assert by['registration_partial_dark']['canReconcile'] and not by['registration_partial_dark']['hasRegister']
    assert not by['registration_lost_light']['canReconcile'] and not by['registration_lost_light']['hasRegister']
    assert paths('registration_lost_light')[register] == paths('registration_fresh_login_light')[register]
    assert paths('register_inflight_dark')[register] == paths('registration_held_fresh_login_dark')[register]
    assert not by['nickname_no_default_light']['hasDefault']
    assert done['fixture']['dropped'] == 3
    assert done['fixture']['paths'][register] == 8
    assert done['fixture']['paths'][verify] == 10
    assert paths('nickname_hint_inflight_dark')[register] == paths('nickname_hint_cancelled_dark')[register] == 8
    assert by['nickname_hint_cancelled_dark']['nicknameLength'] == 0
    login = '/api/w/login/cellphone' if args.legacy else '/api/login/cellphone'
    assert done['fixture']['paths'][login] == 3
    assert done['fixture']['paths'].get('/api/login/token/refresh/v1', 0) == 0
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(states)
    audit = Path(__file__).resolve().parents[2]/'desktop/examples/audit_phone_entry_probe.py'
    subprocess.run([sys.executable, str(audit), str(root/'run'), *(['--legacy'] if args.legacy else [])], check=True)
    report = dict(ok=True, actualWebView=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  legacy=args.legacy, realMusicRequests=0, realAccountWrites=0)
    (root/'report.json').write_text(json.dumps(report, indent=2)+'\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
