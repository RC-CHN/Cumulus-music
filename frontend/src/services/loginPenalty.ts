export interface PenaltyStep {
  kind:
    | 'identity_verify'
    | 'bind_phone'
    | 'real_name'
    | 'upgrade_weak_password'
    | 'upgrade_version'
    | 'unknown';
  typeName: string;
  passed: boolean;
  minVersion: string | null;
}
export interface PenaltyView {
  id: string;
  phase:
    | 'required'
    | 'upgrade_required'
    | 'identity_pending'
    | 'checking'
    | 'read_failed'
    | 'status_passed'
    | 'cancelled';
  code: number | null;
  steps: PenaltyStep[];
  currentStep: number | null;
  boundary:
    | 'official_account_verification'
    | 'official_identity_verification'
    | 'official_bind_phone'
    | 'official_real_name'
    | 'official_password_upgrade'
    | 'official_version_upgrade'
    | 'unsupported_step';
  canCheckStatus: boolean;
}
export function penaltyStepLabel(step: PenaltyStep): string {
  const labels: Record<PenaltyStep['kind'], string> = {
    identity_verify: '验证账号身份',
    bind_phone: '绑定手机',
    real_name: '实名认证',
    upgrade_weak_password: '修改弱密码',
    upgrade_version: '更新官方客户端',
    unknown: '其他账号验证',
  };
  return labels[step.kind];
}
export function penaltyInstruction(view: PenaltyView): string {
  if (view.phase === 'status_passed')
    return '服务端报告的验证步骤已完成，尚需重新登录以确认账号身份。';
  const instructions: Record<PenaltyView['boundary'], string> = {
    official_account_verification: '请在网易云音乐官方客户端完成账号安全验证，再回来核对状态。',
    official_identity_verification: '请在网易云音乐官方客户端验证账号身份，再回来核对状态。',
    official_bind_phone: '请在网易云音乐官方客户端完成绑定手机，再回来核对状态。',
    official_real_name: '请在网易云音乐官方客户端完成实名认证，再回来核对状态。',
    official_password_upgrade: '请在网易云音乐官方客户端修改密码；完成后请使用新密码重新登录。',
    official_version_upgrade: '服务端要求更新官方客户端的验证组件，请先在官方客户端处理。',
    unsupported_step: '此验证步骤暂不能在 Cumulus 中处理，请前往网易云音乐官方客户端。',
  };
  return instructions[view.boundary];
}
