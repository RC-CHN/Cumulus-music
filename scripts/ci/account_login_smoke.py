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
    parser.add_argument('--frontend-dist', type=Path)
    parser.add_argument('--screenshots', action='store_true')
    parser.add_argument('--baseline', action='store_true')
    parser.add_argument('--account-detail', action='store_true')
    parser.add_argument('--profile-edit', action='store_true')
    parser.add_argument('--account-avatar', action='store_true')
    parser.add_argument('--device-removal', action='store_true')
    parser.add_argument('--quick-login', action='store_true')
    parser.add_argument('--active-switch', action='store_true')
    args = parser.parse_args()
    if sum((args.baseline, args.account_detail, args.profile_edit, args.account_avatar, args.device_removal, args.quick_login, args.active_switch)) > 1:
        parser.error('choose one proof mode')
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=False)
    env = {key: value for key, value in os.environ.items() if not key.startswith('CUMULUS_')}
    env['CUMULUS_PROOF_DIR'] = str(root/'run')
    if args.frontend_dist:
        env['CUMULUS_PROOF_FRONTEND_DIST'] = str(args.frontend_dist.resolve(strict=True))
    if args.baseline:
        env['CUMULUS_PROOF_BASELINE'] = '1'
    if args.account_detail:
        env['CUMULUS_PROOF_ACCOUNT_DETAIL'] = '1'
    if args.profile_edit:
        env['CUMULUS_PROOF_PROFILE_EDIT'] = '1'
    if args.account_avatar:
        env['CUMULUS_PROOF_ACCOUNT_AVATAR'] = '1'
    if args.device_removal:
        env['CUMULUS_PROOF_DEVICE_REMOVAL'] = '1'
    if args.quick_login:
        env['CUMULUS_PROOF_QUICK_LOGIN'] = '1'
    if args.active_switch:
        env['CUMULUS_PROOF_ACTIVE_SWITCH'] = '1'
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
    assert len(states) == (8 if args.active_switch else 7 if args.quick_login or args.account_detail else 4 if args.account_avatar else 5 if args.profile_edit else 6 if args.device_removal else 2 if args.baseline else 37)
    for state in states:
        assert not state['overflow']
        if not args.baseline:
            assert state['focused'] and state['focusValid']
        assert all(control['hit'] for control in state['controls'])
        assert state['realMusicRequests'] == 0
    if args.active_switch:
        assert complete['activeSwitch']
        rows = {row['stage'].split('_', 2)[2]: row for row in states}
        eligibility = '/api/login/getquickloginlist'
        switch = '/api/login/switchuser'
        assert [row['theme'] for row in states] == ['light'] * 6 + ['dark'] * 2
        assert [row['fixture']['paths'].get(eligibility, 0) for row in states] == [0, 1, 1, 2, 4, 4, 6, 6]
        assert [row['fixture']['paths'].get(switch, 0) for row in states] == [0, 0, 0, 1, 2, 2, 3, 3]
        assert all(row['nativeAccount']['status'] == 'authenticated' for row in states)
        assert [row['nativeAccount']['nickname'] for row in states] == ['短信验证账号'] * 7 + ['邮箱验证账号']
        source_epoch = states[0]['nativeAccount']['epoch']
        assert all(row['nativeAccount']['epoch'] == source_epoch for row in states[:-1])
        assert states[-1]['nativeAccount']['epoch'] != source_epoch
        assert rows['active_switch_held_light']['fixture']['pending']
        assert not rows['active_switch_cancelled_light']['fixture']['pending']
        assert all(row['fixture']['switchIdentityReads'] == 0 for row in states[:-1])
        assert states[-1]['fixture']['switchIdentityReads'] >= 1
        assert complete['fixture']['paths'][switch] == 3 and complete['fixture']['completed'] == 0
        first_saved = {row['nickname']: row for row in states[0]['savedAccounts']['value']}
        last_saved = {row['nickname']: row for row in states[-1]['savedAccounts']['value']}
        assert first_saved['短信验证账号']['id'] == last_saved['短信验证账号']['id']
        assert first_saved['邮箱验证账号']['id'] != last_saved['邮箱验证账号']['id']
        assert last_saved['邮箱验证账号']['current'] and not last_saved['短信验证账号']['current']
        for row in [*states, complete]:
            paths = row['fixture']['paths']
            assert not any('/quicklogin' in path or '/sms/captcha/' in path or path == '/api/logout' for path in paths)
            serialized = json.dumps(row, ensure_ascii=False)
            assert all(marker not in serialized for marker in ('fixtureSwitch2', 'MUSIC_U=', 'userInfoList', 'fixture-refresh'))
    elif args.quick_login:
        assert complete['quickLogin']
        rows = {row['stage'].split('_', 2)[2]: row for row in states}
        eligibility = '/api/login/getquickloginlist'
        exchange = '/api/login/quicklogin'
        identity = '/api/w/nuser/account/get'
        assert [row['theme'] for row in states] == ['light'] * 5 + ['dark'] * 2
        assert [row['fixture']['paths'].get(eligibility, 0) for row in states] == [1, 1, 2, 2, 3, 5, 5]
        assert [row['fixture']['paths'].get(exchange, 0) for row in states] == [0, 0, 0, 0, 1, 2, 2]
        before = rows['quick_guest_recovery_dark']['fixture']['paths']
        after = rows['quick_guest_adopted_dark']['fixture']['paths']
        assert after[identity] > before[identity]
        assert [row['nativeAccount']['status'] for row in states] == ['authenticated'] + ['signed_out'] * 5 + ['authenticated']
        assert complete['fixture']['paths'].get(exchange) == 2
        for row in [*states, complete]:
            paths = row['fixture']['paths']
            assert not any('/switchuser' in path or '/sms/captcha/' in path or path == '/api/logout' for path in paths)
            serialized = json.dumps(row, ensure_ascii=False)
            assert all(marker not in serialized for marker in ('fixtureQuick2', 'MUSIC_U=', 'userInfoList', 'fixture-refresh'))
    elif args.account_avatar:
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
    elif args.device_removal:
        assert complete['deviceRemoval']
        rows = {state['stage'].split('_', 2)[2]: state for state in states}
        kickoff = '/api/middle/user/security/device/kickoff'
        sms = '/api/sms/captcha/safe/sent'
        listing = '/api/middle/user/device/list'
        assert [row['theme'] for row in states] == ['light'] * 4 + ['dark'] * 2
        assert [(row['width'], row['height']) for row in states[-2:]] == [(880, 670)] * 2
        assert all(row['nativeAccount']['nickname'] == '邮箱验证账号' for row in [*states, complete])
        for name in ['device_removal_current_light', 'device_removal_binding_light']:
            assert rows[name]['fixture']['paths'].get(kickoff, 0) == 0
            assert rows[name]['fixture']['paths'].get(sms, 0) == 0
        assert rows['device_removal_binding_light']['nativeRemoval']['value']['phase'] == 'needs_binding'
        offer = rows['device_removal_sms_offer_light']
        assert offer['nativeRemoval']['value']['phase'] == 'needs_sms'
        assert offer['fixture']['paths'][kickoff] == 1
        assert offer['fixture']['paths'].get(sms, 0) == 0
        assert rows['device_removal_sms_ready_light']['nativeRemoval']['value']['phase'] == 'sms_ready'
        assert rows['device_removal_sms_ready_light']['fixture']['paths'][sms] == 1
        unknown = rows['device_removal_recovery_dark']
        assert unknown['nativeRemoval']['value']['phase'] == 'recovery_required'
        assert unknown['fixture']['deviceRemoved']
        assert unknown['fixture']['paths'][kickoff] == 2
        observed = rows['device_removal_observed_dark']
        assert observed['nativeRemoval']['value']['phase'] == 'observed_absent'
        assert observed['nativeRemoval']['value']['observedPresent'] is False
        # One explicit native reconciliation read, one UI list refresh after confirmed absence.
        assert observed['fixture']['paths'][listing] == unknown['fixture']['paths'][listing] + 2
        assert complete['fixture']['paths'][kickoff] == 2
        assert complete['fixture']['paths'][sms] == 1
        for value in [*states, complete]:
            output_json = json.dumps(value, ensure_ascii=False)
            assert all(marker not in output_json for marker in (
                'fixture-private-key', '1234567890', 'fixture-device-binding-private', '"001234"'
            ))
        database = root/'run/state/core.sqlite3'
        with closing(sqlite3.connect(database.as_uri()+'?mode=ro', uri=True)) as state:
            journal = state.execute(
                'SELECT owner,key,value FROM state WHERE kind=?',
                ('login-device-removal-v1',),
            ).fetchall()
        assert len(journal) == 1
        owner, key, raw = journal[0]
        assert owner == '2' and key == 'active'
        assert json.loads(raw)['phase'] == 'observed_absent'
        assert all(marker not in raw for marker in (
            'fixture-private-key', '1234567890', 'fixture-device-binding-private', '"001234"'
        ))
    elif args.profile_edit:
        assert complete['profileEdit'] and complete['profileRegionsVerified']
        paths = complete['fixture']['paths']
        by_name = {state['stage'].split('_', 2)[2]: state for state in states}
        qr_paths = by_name['binding_qr_light']['fixture']['paths']
        assert qr_paths['/api/w/v1/user/detail/2'] == 1
        assert qr_paths['/api/w/v1/user/bindings/2'] == 1
        assert not any('bindaccount' in path for path in paths)
        assert paths['/api/user/profile/update'] == 2
        assert paths['/api/lbs/region/code'] == 2
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
            region_cache = state.execute(
                'SELECT owner,key,value FROM state WHERE kind=?',
                ('account-profile-regions-v1',),
            ).fetchall()
        assert len(region_cache) == 1 and region_cache[0][:2] == ('2', 'current')
        assert json.loads(region_cache[0][2])['version'] == 'fixture-2026'
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
        assert by_name['devices_ordinary_dark']['theme'] == 'dark'
        paths = complete['fixture']['paths']
        assert paths['/api/w/v1/user/detail/2'] == 1
        assert paths['/api/w/v1/user/bindings/2'] == 1
        assert paths['/api/middle/user/device/list'] == 4
        assert not any(path.endswith(('/kickoff', '/profile/update')) or 'cashier' in path or 'family' in path for path in paths)
    elif not args.baseline:
        by_name = {state['stage'].split('_',2)[2]: state for state in states}
        assert complete['accountLogin']
        assert by_name['email_inflight_small_dark']['fixture']['pending']
        assert by_name['late_login_cancelled_dark']['fixture']['completed'] == 1
        assert by_name['late_login_cancelled_dark']['nativeAccount']['status'] != 'authenticated'
        assert by_name['logout_failed_small_light']['nativeAccount']['status'] == 'authenticated'
        assert by_name['local_logout_light']['nativeAccount']['status'] != 'authenticated'
        unikey, poll = '/api/login/qrcode/unikey', '/api/login/qrcode/client/login'
        error, still, poll_error, generated = (by_name[name] for name in (
            'qr_error_small_dark', 'qr_error_still_small_dark', 'qr_poll_error_small_dark',
            'qr_retry_small_dark'))
        assert error['fixture']['paths'] == still['fixture']['paths'], 'failed generation never retries itself'
        assert poll_error['fixture']['paths'][unikey] == still['fixture']['paths'][unikey] + 1
        assert poll_error['fixture']['paths'][poll] == still['fixture']['paths'].get(poll, 0) + 1
        assert generated['fixture']['paths'][unikey] == poll_error['fixture']['paths'][unikey] + 1
        offer_off, offer_on, revoked, offer_disabled = (by_name[name] for name in (
            'quick_offer_off_dark', 'quick_offer_on_dark', 'quick_offer_invalidated_dark', 'quick_offer_disabled_dark'))
        assert offer_off['savedAccounts'] == offer_on['savedAccounts'] == revoked['savedAccounts'] == offer_disabled['savedAccounts']
        assert offer_off['nativeAccount']['epoch'] == revoked['nativeAccount']['epoch']
        assert revoked['fixture']['paths']['/api/login/getquickloginlist'] == 1
        for endpoint in ('/api/w/v1/user/bindings/2', '/api/w/v1/user/detail/2',
                         '/api/music-vip-membership/client/vip/info', '/api/authentication/identity/user/all'):
            assert offer_on['fixture']['paths'][endpoint] == offer_off['fixture']['paths'].get(endpoint, 0) + 1
            assert revoked['fixture']['paths'][endpoint] == offer_on['fixture']['paths'][endpoint]
            assert offer_disabled['fixture']['paths'][endpoint] == offer_on['fixture']['paths'][endpoint] + 1
        assert offer_disabled['fixture']['paths']['/api/login/getquickloginlist'] == 1
        with sqlite3.connect((root/'run/state/core.sqlite3').as_uri()+'?mode=ro', uri=True) as state:
            offers = dict(state.execute('SELECT key,value FROM state WHERE owner=? AND kind=?',
                                       ('transport', 'quick-login-offer-v1')))
        assert set(offers) == {'2'} and json.loads(offers['2'])['profile'] is None
        current = by_name['qr_error_keeps_current_dark']
        assert current['nativeAccount']['epoch'] == by_name['account_closed_dark']['nativeAccount']['epoch']
        assert current['sessionRenewal']['state'] != 'login_in_progress'
        before, retry, adopted = (by_name[name] for name in (
            'qr_identity_pending_light', 'qr_identity_retry_light', 'qr_authenticated_light'))
        assert before['nativeAccount']['epoch'] == retry['nativeAccount']['epoch']
        assert adopted['nativeAccount']['epoch'] != retry['nativeAccount']['epoch']
        assert all(row['nativeAccount']['status'] != 'authenticated' for row in (before, retry))
        assert adopted['nativeAccount']['status'] == 'authenticated'
        assert [row['fixture']['qrCandidateIdentityReads'] for row in (before, retry, adopted)] == [1, 2, 3]
        for endpoint in ('/api/login/qrcode/client/login', '/api/login/qrcode/unikey'):
            assert len({row['fixture']['paths'][endpoint] for row in (before, retry, adopted)}) == 1

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
                  deviceRemoval=args.device_removal, quickLogin=args.quick_login, activeSwitch=args.active_switch,
                  states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebView=True, realMusicRequests=0, realAccountWrites=0)
    (root/'report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
