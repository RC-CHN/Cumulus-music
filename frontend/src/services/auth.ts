import type { ActiveServerSwitchClient, ServerQuickLoginClient } from './serverQuickLogin';
import type { QuickLoginEligibilityClient } from './quickLoginEligibility';
import type { AccountData, AccountView, QrStatus } from './contracts';
import type { RenewalClient } from './renewal';
import type { LoginPresentationClient } from './loginPresentation';
import type { PenaltyView } from './loginPenalty';
export interface QuickLoginOffer {
  enabled: boolean;
  revision: string | null;
}
export interface QuickLoginAccess {
  assigned: boolean;
  offer: QuickLoginOffer | null;
}
export interface QuickLoginOfferClient {
  quickLoginAccess(epoch: string): Promise<AccountData<QuickLoginAccess>>;
  setQuickLoginOffer(
    epoch: string,
    enabled: boolean,
    expectedRevision: string | null,
  ): Promise<AccountData<QuickLoginOffer>>;
}
export type LoginMode = 'phone_password' | 'phone_sms' | 'email' | 'saved_account';
export type LoginStatus =
  | 'ready'
  | 'submitting'
  | 'sending_sms'
  | 'code_sent'
  | 'loading_verification'
  | 'verifying'
  | 'checking_identity'
  | 'authorized'
  | 'authenticated'
  | 'verification'
  | 'captcha'
  | 'two_factor'
  | 'phone_reuse'
  | 'phone_entry'
  | 'security_question'
  | 'security_qr'
  | 'upgrade_required'
  | 'risk_notice'
  | 'penalty'
  | 'rate_limited'
  | 'account_disabled'
  | 'rejected'
  | 'cancelled'
  | 'error';
export interface LoginAnswer {
  value: string;
  title: string;
  subtitle: string | null;
  picture: string | null;
  tag?: string | null;
}
export interface LoginNoticeView {
  kind: 'risk' | 'upgrade' | 'disabled';
  title: string;
  paragraphs: string[];
}
export interface SecurityQrVerificationView {
  id: string;
  status: QrStatus;
  nickname: string | null;
  modules: boolean[][] | null;
  pollAfterMs: number;
  canRefresh?: boolean;
  canReconcile?: boolean;
}
export interface LoginView {
  penalty?: PenaltyView | null;
  notice?: LoginNoticeView | null;
  securityQr?: SecurityQrVerificationView | null;
  phoneEntry?: PhoneEntryView | null;
  device?: DeviceVerificationView | null;
  sms?: SmsVerificationView | null;
  email?: EmailVerificationView | null;
  question?: QuestionVerificationView | null;
  reuse?: ReuseVerificationView | null;
  operationId: string;
  mode: LoginMode;
  status: LoginStatus;
  code: number | null;
  busy: boolean;
  captchaAvailable: boolean;
  retryAfterMs: number;
  interrupted: boolean;
  challenge: {
    methods: string[];
    nickname: string | null;
    avatarUrl: string | null;
    phoneMask: string | null;
    countryCode?: string | null;
    question: string | null;
    answers: LoginAnswer[];
    reuseConfirmation: boolean;
  };
}
export type LoginCommand =
  | { kind: 'penalty_check'; verification_id: string }
  | { kind: 'security_qr_poll' | 'security_qr_refresh'; verification_id: string }
  | { kind: 'phone_start'; phone: string; country_code: string }
  | { kind: 'phone_retry' | 'phone_resend' | 'phone_nickname_hint'; entry_id: string }
  | { kind: 'phone_code'; entry_id: string; code: string; remember: boolean }
  | { kind: 'phone_register'; entry_id: string; nickname: string | null }
  | { kind: 'sms_start'; direction: 'uplink' | 'downlink' }
  | { kind: 'sms_refresh' | 'sms_check' | 'sms_stop'; verification_id: string }
  | { kind: 'device_start'; method: 'confirm' | 'scan' }
  | { kind: 'device_poll' | 'device_cancel' | 'device_stop'; verification_id: string }
  | { kind: 'device_verify'; verification_id: string; code: string }
  | {
      kind: 'phone_password';
      phone: string;
      country_code: string;
      password: string;
      remember: boolean;
    }
  | { kind: 'phone_sms'; phone: string; country_code: string; code: string; remember: boolean }
  | { kind: 'email'; address: string; password: string; remember: boolean }
  | { kind: 'send_sms'; phone: string; country_code: string }
  | {
      kind:
        | 'verification_methods'
        | 'security_mail_send'
        | 'verification_sms_send'
        | 'question_read'
        | 'reuse_read'
        | 'reconcile';
    }
  | { kind: 'security_mail_verify'; verification_id: string; code: string }
  | { kind: 'security_mail_resend' | 'security_mail_stop'; verification_id: string }
  | { kind: 'verification_sms_verify'; code: string }
  | { kind: 'bind_phone_verify'; phone: string; country_code: string; code: string }
  | { kind: 'question_answer'; question_id: string; answer: string }
  | { kind: 'reuse_choose'; verification_id: string; choice: 'login' | 'register' }
  | {
      kind: 'reuse_back' | 'reuse_confirm' | 'reuse_prepare_register' | 'reuse_nickname_hint';
      verification_id: string;
    }
  | { kind: 'reuse_register'; verification_id: string; nickname: string | null };
export type PhoneEntryStep = 'number' | 'code' | 'nickname' | 'recovery';
export interface PhoneEntryView {
  id: string;
  phase:
    | 'checking_account'
    | 'check_failed'
    | 'sending_code'
    | 'code_required'
    | 'verifying'
    | 'nickname'
    | 'hinting'
    | 'checking_nickname'
    | 'registering'
    | 'recovery_required';
  code: number | null;
  newUser: boolean;
  canSkipNickname: boolean;
  suggestedNickname: string | null;
  nicknameError: ReuseVerificationView['nicknameError'];
  issue:
    | 'check_failed'
    | 'send_failed'
    | 'send_unconfirmed'
    | 'cooldown'
    | 'daily_limit'
    | 'five_per_day'
    | 'code_invalid'
    | 'verification_unconfirmed'
    | 'login_rejected'
    | 'login_unconfirmed'
    | null;
  registrationBlocked: boolean;
  canReconcile: boolean;
}
export interface ReuseVerificationView {
  id: string;
  phase:
    | 'loading'
    | 'choice'
    | 'login_consent'
    | 'register_consent'
    | 'nickname'
    | 'read_failed'
    | 'login_failed'
    | 'confirming'
    | 'hinting'
    | 'checking_nickname'
    | 'registering'
    | 'recovery_required';
  code: number | null;
  maskedNickname: string | null;
  createdAt: number | string | null;
  registrationBlocked: 'fresh_sms_required' | 'unconfirmed_registration' | null;
  canSkipNickname: boolean;
  suggestedNickname: string | null;
  nicknameError:
    | 'too_short'
    | 'too_long'
    | 'invalid_characters'
    | 'taken'
    | 'check_failed'
    | 'rejected'
    | 'sms_invalid'
    | 'phone_registered'
    | 'anti_abuse'
    | 'rate_limited'
    | null;
}
export interface QuestionVerificationView {
  id: string;
  phase: 'loading' | 'ready' | 'verifying' | 'failed' | 'recovery_required';
  questionType: number | null;
  code: number | null;
  answerRejected: boolean;
}
export interface DeviceVerificationView {
  id: string;
  kind: 'confirm' | 'scan';
  phase:
    | 'starting'
    | 'waiting'
    | 'scanned'
    | 'code_required'
    | 'completed'
    | 'expired'
    | 'rejected'
    | 'blocked'
    | 'error'
    | 'recovery_required'
    | 'cancelled'
    | 'stopped'
    | 'stop_unconfirmed';
  code: number | null;
  deviceName: string | null;
  safeDeviceCount: number | null;
  pollAfterMs: number;
  modules: boolean[][] | null;
  answerRejected: boolean;
}
export interface SmsVerificationView {
  id: string;
  direction: 'uplink' | 'downlink';
  phase:
    | 'loading'
    | 'ready'
    | 'code_sent'
    | 'checking'
    | 'failed'
    | 'timed_out'
    | 'recovery_required';
  code: number | null;
  phoneMask: string | null;
  countryCode: string | null;
  uploadText: string | null;
  uploadNumber: string | null;
  answerRejected: boolean;
}
export interface EmailVerificationView {
  id: string;
  phase:
    | 'sending'
    | 'code_required'
    | 'verifying'
    | 'send_limit'
    | 'verify_limit'
    | 'failed'
    | 'send_unconfirmed'
    | 'recovery_required';
  code: number | null;
  mailAddress: string | null;
  answerRejected: boolean;
}
export interface SavedAccount {
  id: string;
  nickname: string;
  avatarUrl: string | null;
  current: boolean;
  autoLogin: boolean;
  removalPending?: boolean;
}
export type LoginCaptchaView =
  | { kind: 'credentials'; value: LoginView }
  | { kind: 'qr'; value: QrStatus };
export interface AuthClient
  extends RenewalClient,
    LoginPresentationClient,
    QuickLoginEligibilityClient,
    QuickLoginOfferClient,
    ServerQuickLoginClient,
    ActiveServerSwitchClient {
  solveLoginCaptcha(operationId: string): Promise<LoginCaptchaView>;
  onAccountChanged(handler: (account: AccountView) => void): Promise<() => void>;
  beginLogin(epoch: string, mode: LoginMode, savedId?: string): Promise<LoginView>;
  loginSnapshot(operationId: string): Promise<LoginView>;
  loginCommand(operationId: string, command: LoginCommand): Promise<LoginView>;
  cancelLogin(operationId: string): Promise<void>;
  savedAccounts(epoch: string): Promise<AccountData<SavedAccount[]>>;
  removeSavedAccount(epoch: string, id: string): Promise<void>;
  logoutAccount(
    epoch: string,
    remote: boolean,
  ): Promise<{ account: AccountView; remoteRevoked: boolean }>;
}
export function loginMessage(view: LoginView | undefined): string {
  if (!view) return '正在准备登录…';
  if (view.status === 'rejected') {
    if ([501, 502].includes(view.code ?? 0)) return '账号或密码错误，请重新输入';
    return '登录未通过，请检查输入后重试';
  }
  const messages: Record<LoginStatus, string> = {
    ready: '',
    submitting: '正在登录…',
    sending_sms: '正在发送验证码…',
    code_sent: '验证码已发送',
    loading_verification: '正在获取验证方式…',
    verifying: '正在验证…',
    checking_identity: '正在核对账号…',
    authorized: '正在核对账号…',
    authenticated: '登录成功',
    verification: '此账号需要网页安全验证，当前尚未接入，请在官方 App 完成验证后重新扫码',
    captcha: view.captchaAvailable
      ? '请打开安全验证窗口，完成验证码后继续登录'
      : '此步骤需要额外验证，请改用扫码登录',
    two_factor: '请选择安全验证方式',
    phone_reuse: '需要确认此手机号关联的账号',
    phone_entry: '',
    security_question: '请回答账号安全问题',
    security_qr: '请使用已登录该账号的设备扫码验证',
    upgrade_required: '版本更新提醒',
    risk_notice: '登录风险提示',
    penalty: '账号安全确认',
    rate_limited: '操作过于频繁，请稍后再试',
    account_disabled: '用户禁止登录',
    rejected: '',
    cancelled: '已取消登录',
    error: '登录结果未确认，可以先核对登录状态',
  };
  return messages[view.status];
}
