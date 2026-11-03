"""Production notice/penalty/security-QR WebView and IPC with native synthetic I/O."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sqlite3
import time
from window_manager import launch


STAGES = [
    'guest_ready_light',
    'risk_8810_light',
    'risk_10004_long_small_dark',
    'risk_10004_scrolled_small_dark',
    'disabled_304_small_dark',
    'upgrade_8820_light',
    'upgrade_8810_small_dark',
    'penalty_upgrade_light',
    'penalty_identity_small_dark',
    'penalty_unknown_small_dark',
    'penalty_failed_small_dark',
    'penalty_retried_small_dark',
    'penalty_passed_301_small_dark',
    'penalty_restart_small_dark',
    'security_qr_seed_light',
    'security_qr_802_light',
    'security_qr_expired_small_dark',
    'security_qr_refreshed_small_dark',
    'security_qr_captcha_small_dark',
    'security_qr_captcha_resumed_small_dark',
    'security_qr_late_cancelled_small_dark',
    'security_qr_identity_pending_light',
    'security_qr_adopted_light',
]


def windows_capture(binary, env, log, output):
    """Capture only on an explicitly requested isolated Windows GUI runner."""
    from PIL import ImageGrab
    child_env = {**env, 'CUMULUS_PROOF_CAPTURE_ACK': '1'}
    process = subprocess.Popen([str(binary)], env=child_env, stdout=log, stderr=log)
    seen = set()
    try:
        deadline = time.monotonic() + 180
        while process.poll() is None:
            if time.monotonic() >= deadline:
                raise TimeoutError('login notice WebView did not finish')
            for path in sorted(output.glob('ux_*.json')):
                if path in seen:
                    continue
                try:
                    json.loads(path.read_text(encoding='utf-8'))
                except json.JSONDecodeError:
                    continue
                ImageGrab.grab(all_screens=True).save(path.with_suffix('.png'))
                path.with_suffix('.captured').write_text('captured\n', encoding='utf-8')
                seen.add(path)
            time.sleep(.08)
        return process.returncode
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


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
    env['CUMULUS_PROOF_DIR'] = str(root / 'run')
    for key, name in [
        ('XDG_DATA_HOME', 'data'), ('XDG_CONFIG_HOME', 'config'), ('XDG_CACHE_HOME', 'cache'),
        ('XDG_RUNTIME_DIR', 'runtime'), ('APPDATA', 'roaming'), ('LOCALAPPDATA', 'local'),
    ]:
        directory = root / 'os-profile' / name
        directory.mkdir(parents=True, mode=0o700)
        env[key] = str(directory)
    if args.runtime:
        env['PATH'] = str(args.runtime.resolve()) + os.pathsep + env.get('PATH', '')
    output = root / 'run' / 'first'
    with (root / 'host.log').open('x', encoding='utf-8') as log:
        if os.name == 'nt' and args.screenshots:
            code = windows_capture(args.binary.resolve(), env, log, output)
        else:
            code = launch(args.binary.resolve(), env, log, output, args.screenshots)
    failure = output / 'failed.json'
    assert not failure.exists(), failure.read_text(encoding='utf-8') if failure.exists() else ''
    assert code == 0, f'login notice process exited {code}'
    complete = json.loads((output / 'complete.json').read_text(encoding='utf-8'))
    paths = sorted(output.glob('ux_*.json'))
    states = [json.loads(path.read_text(encoding='utf-8')) for path in paths]
    assert complete['ok'] and complete['loginNotices'] and complete['windowsDeclaration']
    assert complete['syntheticDeviceContext'] and complete['nativeClientSignPresent'] is False
    assert complete['stageCount'] == len(states) == len(STAGES)
    assert [state['stage'].split('_', 2)[2] for state in states] == STAGES
    by_name = dict(zip(STAGES, states))
    for index, state in enumerate(states):
        assert not state['overflow'] and state['focused'] and state['focusValid']
        assert state['focusVisible'], f"focus outside visible content: {state['stage']}"
        assert all(control['hit'] for control in state['controls'])
        assert state['realMusicRequests'] == 0 and state['nativeCaptchaProvider']
        assert state['syntheticDeviceContext'] and state['nativeClientSignPresent'] is False
        assert state['windowsDeclaration'] and isinstance(state['nativeClientSignPresent'], bool)
        assert state['fixture']['unexpectedRequests'] == 0
        assert state['passwordLength'] == 0
        account = state['nativeAccount']
        saved = state['savedAccounts']['value']
        if index == 0:
            assert account['status'] != 'authenticated' and not saved
        elif index == len(states) - 1:
            assert account['status'] == 'authenticated' and account['nickname'] == '短信验证账号'
            assert len(saved) == 2
            assert sum(record['current'] for record in saved) == 1
            assert next(record for record in saved if record['current'])['nickname'] == '短信验证账号'
        else:
            assert account['status'] == 'authenticated' and account['nickname'] == '邮箱验证账号'
            assert len(saved) == 1 and saved[0]['current'] and saved[0]['nickname'] == '邮箱验证账号'
        for marker in ['NATIVE_NOTICE_PRIVATE', 'NATIVE_PENALTY_PRIVATE', 'NATIVE_QR_PRIVATE',
                       'synthetic-native-only-', 'synthetic-penalty-tu']:
            assert marker not in json.dumps(state, ensure_ascii=False)
    risk = by_name['risk_8810_light']
    assert risk['noticeKind'] == 'risk' and risk['noticeTitle'] == '合成账号风险说明'
    assert len(risk['noticeParagraphs']) == 2 and risk['code'] == 8810
    long_risk = by_name['risk_10004_long_small_dark']
    assert long_risk['theme'] == 'dark' and long_risk['width'] == 760 and long_risk['height'] == 540
    assert long_risk['code'] == 10004 and len(long_risk['noticeParagraphs']) == 9
    assert long_risk['noticeParagraphs'][0] == '<strong>合成文字，应作为正文显示</strong>'
    assert by_name['risk_10004_scrolled_small_dark']['scrollMoved']
    assert by_name['risk_10004_scrolled_small_dark']['scrollTop'] > 20
    assert by_name['disabled_304_small_dark']['noticeTitle'] == '用户禁止登录'
    assert by_name['disabled_304_small_dark']['noticeKind'] == 'disabled'
    assert by_name['upgrade_8820_light']['noticeKind'] == 'upgrade'
    assert by_name['upgrade_8810_small_dark']['noticeKind'] == 'upgrade'
    assert by_name['penalty_upgrade_light']['penaltyCurrent'] == '更新官方客户端'
    assert '验证账号身份' in by_name['penalty_identity_small_dark']['penaltyText']
    assert len(by_name['penalty_unknown_small_dark']['penaltySteps']) == 32
    assert by_name['penalty_unknown_small_dark']['stepsScrolled']
    assert not by_name['penalty_unknown_small_dark']['dialogScrollable']
    assert '暂不能在 Cumulus' in by_name['penalty_retried_small_dark']['penaltyText']
    assert all(step['passed'] for step in by_name['penalty_passed_301_small_dark']['penaltySteps'])
    assert by_name['penalty_restart_small_dark']['noPasswordReplay']
    assert by_name['security_qr_seed_light']['qrRendered']
    assert by_name['security_qr_seed_light']['fixture']['legacyCreates'] == 0
    assert by_name['security_qr_802_light']['securityQrStatus'] == 'waiting_confirmation'
    assert by_name['security_qr_expired_small_dark']['securityQrStatus'] == 'expired'
    assert by_name['security_qr_refreshed_small_dark']['qrRendered']
    assert by_name['security_qr_refreshed_small_dark']['fixture']['legacyCreates'] == 1
    assert by_name['security_qr_captcha_small_dark']['securityQrStatus'] == 'challenge'
    resumed = by_name['security_qr_captcha_resumed_small_dark']
    assert resumed['qrRendered'] and resumed['fixture']['captchaCalls'] == resumed['fixture']['proofPolls'] == 1
    late = by_name['security_qr_late_cancelled_small_dark']
    assert late['fixture']['cancelledReplies'] == 1 and late['fixture']['pending'] is None
    held = by_name['security_qr_identity_pending_light']
    assert held['fixture']['pending'] == 'identity_read' and held['fixture']['identityAuthorized'] == 1
    fixture = complete['fixture']
    assert fixture['loginCodes'] == [200, 8810, 10004, 304, 8820, 8810, 8831, 8831, 8831, 8810, 8810]
    assert fixture['browserDocuments'] == ['client_release', 'client_release', 'app_download']
    assert fixture['captchaCalls'] == fixture['proofPolls'] == fixture['legacyCreates'] == 1
    assert fixture['identity301'] == fixture['identityAuthorized'] == fixture['cancelledReplies'] == 1
    assert fixture['released'] == 2 and fixture['pending'] is None and fixture['unexpectedRequests'] == 0
    requests = fixture['paths']
    assert requests.get('/api/login') == 11
    assert requests.get('/api/login/penalty/status') == 3
    assert requests.get('/api/login/token/refresh/v1') == 1
    assert fixture['modePaths']['qr_authorize'].get('/api/login/token/refresh/v1') == 1
    for state in states[:-1]:
        assert state['fixture']['paths'].get('/api/login/token/refresh/v1', 0) == 0
    for mode in ['seed_old', 'risk_8810', 'risk_10004', 'disabled', 'upgrade_8820', 'upgrade_8810',
                 'penalty_upgrade', 'penalty_identity', 'penalty_unknown', 'qr_seed', 'qr_authorize']:
        assert fixture['modePaths'][mode].get('/api/login') == 1, mode
    for mode in ['penalty_status_fail', 'penalty_status_unknown', 'penalty_status_passed']:
        assert fixture['modePaths'][mode].get('/api/login/penalty/status') == 1, mode
    assert fixture['modePaths']['penalty_status_passed'].get('/api/w/nuser/account/get') == 1
    for endpoint in ['/api/w/login', '/api/w/login/cellphone', '/api/login/cellphone', '/api/logout',
                     '/api/sms/captcha/sent', '/api/user/2fa/bindcellphone', '/api/user/certificate/safe/get']:
        assert requests.get(endpoint, 0) == 0, endpoint
    # Automatic poll timing can differ across actual WebViews and screenshots.
    # Terminal responses, explicit refresh, CAPTCHA and all credential writes
    # have exact counts; waiting/802 polls are bounded and accounted by mode.
    legacy = fixture['legacyModePolls']
    assert set(legacy) <= {'qr_seed', 'qr_802', 'qr_expired', 'qr_refresh', 'qr_captcha', 'qr_hold', 'qr_authorize'}
    assert sum(legacy.values()) == fixture['legacyPolls']
    assert legacy.get('qr_expired') == legacy.get('qr_hold') == legacy.get('qr_authorize') == 1
    assert 1 <= legacy.get('qr_802', 0) <= 16 and 2 <= legacy.get('qr_captcha', 0) <= 16
    assert all(0 < value <= 16 for value in legacy.values())
    assert all(complete[key] for key in ['returnedToPhone', 'noPasswordReplay', 'nativeProofOnly',
                                       'lateReplyCancelled', 'identityReadBeforeAdoption'])
    if args.screenshots:
        assert len(list(output.glob('ux_*.png'))) == len(states)
        assert len(list(output.glob('ux_*.captured'))) == len(states)
        assert all(path.with_suffix('.png').stat().st_size > 0 for path in paths)
    # A projection containing only booleans is insufficient: the recursive
    # failure archive also includes native SQLite state. Never persist actual
    # installation hardware facts anywhere under an account UI proof root.
    for database in (root / 'run' / 'state').rglob('core.sqlite3'):
        with sqlite3.connect(database.resolve().as_uri() + '?mode=ro', uri=True) as connection:
            assert connection.execute(
                'SELECT 1 FROM state WHERE owner=? AND kind=? LIMIT 1',
                ('device', 'identity'),
            ).fetchone() is None, 'hardware identity must stay outside synthetic account proofs'
    report = dict(ok=True, states=len(states), screenshots=len(states) if args.screenshots else 0,
                  actualWebView=True, nativeCaptchaProvider=True, realMusicRequests=0,
                  realAccountWrites=0, oldAccountRetainedUntilIdentity=True, noPasswordReplay=True,
                  windowsDeclaration=True, nativeClientSignPresent=False, syntheticDeviceContext=True,
                  persistedHardwareIdentity=False)
    (root / 'report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(report))


if __name__ == '__main__':
    main()
