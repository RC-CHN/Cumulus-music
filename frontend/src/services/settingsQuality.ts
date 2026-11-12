import { qualities, type Quality } from './player.ts';

export type DefaultQualityAccess = 'allowed' | 'requires_vip' | 'requires_svip' | 'unknown';
export interface DefaultQualityView {
  epoch: string;
  revision: string | null;
  quality: Quality;
  damaged?: boolean;
  choices: { quality: Quality; access: DefaultQualityAccess }[];
}

export interface PlaybackSettingsQualityClient {
  playbackQualityPreferences(epoch: string, refresh: boolean): Promise<DefaultQualityView>;
  savePlaybackQualityPreference(
    epoch: string,
    revision: string,
    quality: Quality,
  ): Promise<DefaultQualityView>;
}

// M/40 tagSelectionMeta. Native decides which tiers to offer and in which order.
const labels: Partial<Record<Quality, string>> = {
  vivid: '臻音全景声',
  sky: '沉浸环绕声',
  jymaster: '超清母带',
  jyeffect: '高清臻音',
  hires: '高解析度无损',
  lossless: '无损',
  exhigh: '极高',
  standard: '标准',
};
export function settingsQualityLabel(quality: Quality) {
  return labels[quality] ?? qualities.find((entry) => entry.value === quality)?.label ?? quality;
}
export const settingsQualityAccessLabel: Record<DefaultQualityAccess, string> = {
  allowed: '',
  requires_vip: '需黑胶 VIP',
  requires_svip: '需黑胶 SVIP',
  unknown: '权益待核对',
};
export function displayedSettingsQuality(quality: Quality | undefined) {
  return quality === 'higher' ? 'standard' : quality;
}
export function validateSettingsQuality(view: DefaultQualityView, epoch: string) {
  if (
    view?.epoch !== epoch ||
    !(view.revision === null || (typeof view.revision === 'string' && view.revision.length > 0)) ||
    !qualities.some((row) => row.value === view.quality) ||
    !Array.isArray(view.choices) ||
    view.choices.length > 8 ||
    new Set(view.choices.map((row) => row?.quality)).size !== view.choices.length ||
    view.choices.some(
      (row) =>
        !row ||
        !Object.hasOwn(labels, row.quality) ||
        !Object.hasOwn(settingsQualityAccessLabel, row.access),
    )
  )
    throw { code: 'invalid_quality_settings', message: '未能核对当前音质设置。' };
  return view;
}
