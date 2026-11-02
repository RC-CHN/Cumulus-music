import type { ReuseVerificationView } from './auth';
export function reuseTitle(phase: ReuseVerificationView['phase'] | undefined): string {
  if (phase && ['login_consent', 'login_failed', 'confirming'].includes(phase))
    return '登录当前账号';
  if (phase === 'register_consent') return '注册新账号';
  if (phase && ['hinting', 'nickname', 'checking_nickname', 'registering'].includes(phase))
    return '希望大家怎么称呼你呢？';
  return '确认账号';
}
export function reuseDate(value: number | string | null | undefined): string {
  if (value == null || value === '') return '';
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return '';
  // Preserve date-only calendar values; ISO timestamps use the local calendar,
  // matching official M103/dayjs's default YYYY-MM-DD formatter.
  if (typeof value === 'string' && /^\d{4}-\d{2}-\d{2}$/.test(value)) return value;
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}
export function nicknameError(value: ReuseVerificationView['nicknameError']): string {
  return value
    ? {
        too_short: '昵称不能少于4个英文或2个汉字',
        too_long: '字数超出限制',
        invalid_characters: '昵称只支持中英文、数字和-_两个符号',
        taken: '昵称已使用',
        check_failed: '暂时无法检查昵称，请重试',
        rejected: '注册未完成，请检查昵称或重新获取短信验证码',
        sms_invalid: '验证码错误，请重新短信登录',
        phone_registered: '手机号已经被注册，请返回确认关联账号',
        anti_abuse: '注册未通过安全检查，请稍后再试',
        rate_limited: '操作过于频繁，请稍后再试',
      }[value]
    : '';
}
