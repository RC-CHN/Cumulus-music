export type RenewalState =
  | 'unavailable'
  | 'signed_out'
  | 'expired'
  | 'login_in_progress'
  | 'policy_disabled'
  | 'ready'
  | 'checking'
  | 'missing_credentials'
  | 'not_due'
  | 'refreshed'
  | 'recovery_required'
  | 'reauthentication_required'
  | 'failed';
export interface SessionRenewalView {
  epoch: string;
  state: RenewalState;
  lastSuccess: number | null;
  busy: boolean;
  canCheck: boolean;
  canReconcile: boolean;
}
export interface RenewalClient {
  sessionRenewal(epoch: string): Promise<SessionRenewalView>;
  renewSession(epoch: string, action: 'check' | 'reconcile'): Promise<SessionRenewalView>;
  onSessionRenewal(handler: (view: SessionRenewalView) => void): Promise<() => void>;
}
export function renewalMessage(state: RenewalState): string {
  const messages: Record<RenewalState, string> = {
    unavailable: '此运行环境未启用登录续期',
    signed_out: '登录后可检查续期状态',
    expired: '登录已失效，请重新登录',
    login_in_progress: '请先完成或取消正在进行的登录',
    policy_disabled: '服务暂未为此账号启用自动续期，登录失效后需重新登录',
    ready: '回到应用时会自动检查登录续期',
    checking: '正在检查登录续期…',
    missing_credentials: '本次登录未提供续期凭据，可以继续使用；登录失效后需重新登录',
    not_due: '登录已续期，距上次成功不足 24 小时',
    refreshed: '登录续期成功，账号身份已核对',
    recovery_required: '上次续期结果尚未确认，请先核对结果',
    reauthentication_required: '无法确认上次续期，请重新登录；仍可重试核对结果',
    failed: '续期检查未完成，请稍后重试',
  };
  return messages[state];
}
