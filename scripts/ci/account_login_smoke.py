"""Actual production login/account WebView, isolated credentials and synthetic EAPI only."""
import argparse
import json
import os
import sqlite3
from contextlib import closing
from pathlib import Path
from window_manager import launch


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--runtime', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--baseline', action='store_true')
    parser.add_argument('--account-detail', action='store_true')
    parser.add_argument('--profile-edit', action='store_true')
    parser.add_argument('--account-avatar', action='store_true')
    args = parser.parse_args()
    if sum((args.baseline, args.account_detail, args.profile_edit, args.account_avatar)) > 1:
        parser.error('choose one proof mode')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    if args.baseline:
        env['CUMULUS_PROOF_BASELINE'] = '1'
    if args.account_detail:
        env['CUMULUS_PROOF_ACCOUNT_DETAIL'] = '1'
    if args.profile_edit:
        env['CUMULUS_PROOF_PROFILE_EDIT'] = '1'
    if args.account_avatar:
        env['CUMULUS_PROOF_ACCOUNT_AVATAR'] = '1'
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
    assert len(states) == (4 if args.account_avatar else 5 if args.profile_edit else 6 if args.account_detail else 2 if args.baseline else 28)
    for state in states:
        assert not state['overflow']
        if not args.baseline:
            assert state['focused'] and state['focusValid']
        assert all(control['hit'] for control in state['controls'])
        assert state['realMusicRequests'] == 0
    if args.account_avatar:
        assert complete['accountAvatar']
        by_name = {state['stage'].split('_', 2)[2]: state for state in states}
        paths = complete['fixture']['paths']
        assert complete['fixture']['avatarObjects'] == 2 and complete['fixture']['avatarChunks'] >= 2
        assert paths['/api/nos/token/alloc'] == paths['/api/user/avatar/upload/v1'] == 2
        assert paths.get('/api/user/profile/update', 0) == 0
        before = by_name['avatar_recovery_dark']['fixture']['paths']
        assert before['/api/user/avatar/upload/v1'] == 2
        assert paths['/api/w/v1/user/detail/2'] > before['/api/w/v1/user/detail/2']
        database = root/'run/state/core.sqlite3'
        with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True)) as state:
            rows = state.execute(
                'SELECT owner,value FROM state WHERE kind=?', ('account-avatar-v1',),
            ).fetchall()
        assert len(rows) == 2
        for owner, raw in rows:
            task = json.loads(raw)
            assert owner == '2' and task['phase'] == 'confirmed'
            assert not task['source'] and not task['encoded']
        assert not list((root/'run/state').glob('account-avatar-*.jpg'))
    elif args.profile_edit:
        assert complete['profileEdit']
        paths = complete['fixture']['paths']
        by_name = {state['stage'].split('_', 2)[2]: state for state in states}
        qr_paths = by_name['binding_qr_light']['fixture']['paths']
        assert qr_paths['/api/w/v1/user/detail/2'] == 1
        assert qr_paths['/api/w/v1/user/bindings/2'] == 1
        assert not any('bindaccount' in path for path in paths)
        assert paths['/api/user/profile/update'] == 2
        assert by_name['profile_edit_saved_light']['fixture']['paths']['/api/user/profile/update'] == 1
        before = by_name['profile_edit_recovery_dark']['fixture']['paths']
        assert before['/api/user/profile/update'] == 2
        assert paths['/api/w/v1/user/detail/2'] > before['/api/w/v1/user/detail/2']
        database = root/'run/state/core.sqlite3'
        with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True)) as state:
            rows = state.execute(
                'SELECT owner,key,value FROM state WHERE kind=?',
                ('account-profile-edit-intents-v1',),
            ).fetchall()
        assert len(rows) == 2
        signatures = set()
        for owner, key, raw in rows:
            intent = json.loads(raw)
            assert owner == '2' and intent['id'] == key and intent['phase'] == 'confirmed'
            assert set(intent['submitted']) == {'signature'}
            signature = intent['submitted']['signature']
            assert intent['observed']['profile']['signature'] == signature
            signatures.add(signature)
        assert signatures == {'桌面验证：已保存', '桌面验证：回包丢失'}
    elif args.account_detail:
        assert complete['accountDetail']
        by_name = {state['stage'].split('_', 2)[2]: state for state in states}
        assert by_name['devices_loading_light']['fixture']['pending']
        assert by_name['devices_ready_light']['fixture']['completed'] == 1
        paths = complete['fixture']['paths']
        assert paths['/api/w/v1/user/detail/2'] == 1
        assert paths['/api/w/v1/user/bindings/2'] == 1
        assert paths['/api/middle/user/device/list'] == 3
        assert not any(path.endswith(('/kickoff', '/profile/update')) for path in paths)
    elif not args.baseline:
        by_name = {state['stage'].split('_',2)[2]: state for state in states}
        assert complete['accountLogin']
        assert by_name['email_inflight_small_dark']['fixture']['pending']
        assert by_name['late_login_cancelled_dark']['fixture']['completed'] == 1
        assert by_name['late_login_cancelled_dark']['nativeAccount']['status'] != 'authenticated'
        assert by_name['logout_failed_small_light']['nativeAccount']['status'] == 'authenticated'
        assert by_name['local_logout_light']['nativeAccount']['status'] != 'authenticated'
        paths = complete['fixture']['paths']
        assert paths.get('/api/login/token/refresh/v1', 0) == 0  # Fresh login starts the daily interval.
        renewal = by_name['saved_current_small_light']['sessionRenewal']
        assert renewal['state'] == 'not_due' and renewal['lastSuccess'] is None
        assert paths.get('/api/sms/captcha/sent') == 1
        assert paths.get('/api/logout') == 1  # Local-only fallback sends no second request.
        assert any(account['current'] and not account['autoLogin'] and account['nickname'] == '短信验证账号'
                   for account in complete['savedAccounts']['value'])
        assert paths.get('/api/w/login', 0) == paths.get('/api/w/login/cellphone', 0) == 0
        assert paths.get('/api/login/cellphone') == 2
        assert paths.get('/api/login') == 3  # Wrong, cancelled inflight, explicit successful retry.
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(states)
    report = dict(ok=True, baseline=args.baseline, accountDetail=args.account_detail, profileEdit=args.profile_edit,
                  accountAvatar=args.account_avatar,
                  states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebView=True, realMusicRequests=0, realAccountWrites=0)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
