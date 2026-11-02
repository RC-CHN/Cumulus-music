import type { AccountData, AccountView, QrStatus } from './contracts';
import type { RenewalClient } from './renewal';
import type { LoginPresentationClient } from './loginPresentation';
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
  | 'security_question'
  | 'security_qr'
  | 'upgrade_required'
  | 'risk_notice'
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
export interface LoginView {
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
export interface AuthClient extends RenewalClient, LoginPresentationClient {
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
    security_question: '请回答账号安全问题',
    security_qr: '此账号需要原设备安全验证，当前尚未接入，请在官方 App 完成验证后重新扫码',
    upgrade_required: '服务要求更新登录组件，请暂时使用官方 App',
    risk_notice: '账号需要安全检查，请先在官方 App 查看并处理',
    rate_limited: '操作过于频繁，请稍后再试',
    account_disabled: '该账号暂时无法登录，请在官方 App 查看账号状态',
    rejected: '',
    cancelled: '已取消登录',
    error: '登录结果未确认，可以先核对登录状态',
  };
  return messages[view.status];
}
