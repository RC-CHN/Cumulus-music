import {
  qualities,
  type Quality,
  type QualityChoiceView,
  type QualityTrialView,
} from './player.ts';
export const qualityOrder: Quality[] = [
  'vivid',
  'sky',
  'jymaster',
  'jyeffect',
  'hires',
  'lossless',
  'exhigh',
  'standard',
  'higher',
  'dolby',
  '64aac',
];
export const qualityDescriptions: Record<Quality, string> = {
  vivid: 'Audio Vivid · 最高 7.1 声道',
  sky: '最高 5.1 声道',
  jymaster: '最高 192 kHz / 24 bit',
  jyeffect: '最高 96 kHz / 24 bit',
  hires: '最高 192 kHz / 24 bit',
  lossless: '最高 48 kHz / 16 bit',
  exhigh: '320 kbps',
  standard: '128 kbps',
  higher: '192 kbps · 兼容音质',
  dolby: '杜比音源 · 兼容音质',
  '64aac': '64 kbps · 兼容音质',
};
export const qualityLabel = (value?: string | null) =>
  qualities.find((q) => q.value === value)?.label ?? (value ? '未知音质' : '未播放');
export function qualitySize(value?: string | null) {
  if (!value || !/^\d+$/.test(value)) return '';
  const bytes = BigInt(value);
  if (bytes === 0n) return '';
  let unit = 1n,
    index = 0;
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB', 'EB'];
  while (index < units.length - 1 && bytes >= unit * 1024n) {
    unit *= 1024n;
    index++;
  }
  const tenths = (bytes * 10n + unit / 2n) / unit;
  return `${tenths / 10n}${tenths % 10n ? `.${tenths % 10n}` : ''} ${units[index]}`;
}
export function qualityDuration(ms?: number | null) {
  if (ms == null || !Number.isFinite(ms) || ms <= 0) return '';
  const seconds = Math.ceil(ms / 1000);
  if (seconds >= 3600)
    return `${Math.floor(seconds / 3600)} 小时${Math.floor((seconds % 3600) / 60) ? ` ${Math.floor((seconds % 3600) / 60)} 分钟` : ''}${seconds % 60 ? ` ${seconds % 60} 秒` : ''}`;
  return seconds >= 60
    ? `${Math.floor(seconds / 60)} 分钟${seconds % 60 ? ` ${seconds % 60} 秒` : ''}`
    : `${seconds} 秒`;
}
export function qualityBadge(choice?: QualityChoiceView, trial?: QualityTrialView) {
  if (trial && ['sending', 'uncertain', 'acknowledged'].includes(trial.phase)) return '待核对';
  if (!choice) return '';
  return {
    allowed: '',
    trial_eligible: '可试用',
    trial_active: '试用中',
    requires_vip: '黑胶 VIP',
    requires_svip: '超级会员',
    requires_purchase: '需购买',
    unavailable: '暂无音源',
    trial_unsupported: '试听不支持',
    unknown: '权益待确认',
  }[choice.access];
}
export function qualityUnavailable(choice?: QualityChoiceView) {
  return choice?.availability === 'unavailable' || choice?.access === 'trial_unsupported';
}
