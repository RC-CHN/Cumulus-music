import type { AccountData, AccountView, QrStatus } from './contracts';
import type { RenewalClient } from './renewal';
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
}
export interface LoginView {
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
    question: string | null;
    answers: LoginAnswer[];
    reuseConfirmation: boolean;
  };
}
export type LoginCommand =
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
        | 'reuse_confirm'
        | 'reconcile';
    }
  | { kind: 'security_mail_verify' | 'verification_sms_verify'; code: string }
  | { kind: 'bind_phone_verify'; phone: string; country_code: string; code: string }
  | { kind: 'question_answer'; answer: string };
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
export interface AuthClient extends RenewalClient {
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
