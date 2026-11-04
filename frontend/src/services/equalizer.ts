import { equalizerPresets } from '../data/equalizer-presets.ts';
import type {
  EqualizerView,
  EqualizerSettings,
  EqualizerPreset,
  PlayerAction,
  BoundParametricStatus,
} from './player.ts';
export { equalizerPresets };
export const equalizerLabels = ['31', '63', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'];
/** Show the same hundredth of a dB as the native range, without float32 tails. */
export function equalizerGainLabel(gain: number): string {
  return String(Number(gain.toFixed(2)));
}
export function equalizerPreset(gains: readonly number[]): number {
  return equalizerPresets.findIndex(
    (p) =>
      p.gains.length === gains.length && p.gains.every((g, i) => Math.abs(g - gains[i]!) < 0.00001),
  );
}
export function equalizerPresetOption(
  preset: EqualizerPreset | null | undefined,
  gains: readonly number[],
  saved?: readonly number[] | null,
) {
  if (preset?.kind === 'built_in') return preset.index;
  if (preset?.kind === 'manual') return -1;
  if (preset?.kind === 'saved') return -2;
  if (saved?.every((gain, i) => Math.fround(gain) === Math.fround(gains[i]!))) return -2;
  return equalizerPreset(gains);
}
export function equalizerTone(settings?: EqualizerSettings) {
  return { bass_db: settings?.bass_db ?? 0, treble_db: settings?.treble_db ?? 0 };
}
export function equalizerToneEnabled(settings?: EqualizerSettings) {
  return settings?.tone_enabled ?? settings?.enabled ?? false;
}
function activeGeq(settings: EqualizerSettings) {
  return settings.enabled && settings.gains.some((gain) => gain !== 0);
}
function activeTone(settings: EqualizerSettings) {
  return (
    equalizerToneEnabled(settings) && Object.values(equalizerTone(settings)).some((g) => g !== 0)
  );
}
export function equalizerChange(
  view: EqualizerView,
  settings: EqualizerSettings,
  preset?: EqualizerPreset,
): Extract<PlayerAction, { kind: 'equalizer' }> {
  const tone = equalizerTone(settings);
  if (
    Object.values(tone).some((gain) => !Number.isInteger(gain) || Math.abs(gain) > 10) ||
    view.damaged ||
    settings.gains.length !== 10 ||
    settings.gains.some((g) => !Number.isFinite(g) || Math.abs(g) > 12)
  )
    throw new Error('均衡器参数无效');
  return {
    kind: 'equalizer',
    expected_revision: view.revision,
    ...(preset ? { preset } : {}),
    settings: {
      enabled: settings.enabled,
      ...(settings.master_bypassed ? { master_bypassed: true } : {}),
      gains: [...settings.gains],
      ...(settings.tone_enabled !== undefined ? { tone_enabled: settings.tone_enabled } : {}),
      ...(tone.bass_db !== 0 ? { bass_db: tone.bass_db } : {}),
      ...(tone.treble_db !== 0 ? { treble_db: tone.treble_db } : {}),
    },
  };
}
function equalizerOff(settings: EqualizerSettings) {
  return !settings.enabled && !activeTone(settings);
}

export function equalizerCanRetry(view: EqualizerView): boolean {
  return (
    view.status === 'failed' ||
    (view.status === 'unavailable' &&
      !view.settings.master_bypassed &&
      (activeGeq(view.settings) || activeTone(view.settings)))
  );
}

export function equalizerMessage(view: EqualizerView): string {
  if (view.damaged) return '本地均衡器设置损坏，请修复后重试。';
  if (view.status === 'failed' && view.settings.master_bypassed)
    return '设置已保存，但未能确认音效已暂停。可重新应用。';
  if (view.status === 'failed' && equalizerOff(view.settings))
    return '设置已保存，但未能确认均衡器已关闭。可重新应用。';
  if (view.status === 'failed')
    return activeTone(view.settings)
      ? '设置已保存，但音效未能应用。可重新应用或重置低音和高音。'
      : '设置已保存，但均衡器未能应用。可重新应用，或关闭后继续播放。';
  if (view.status === 'unavailable' && equalizerCanRetry(view))
    return '均衡器组件未就绪，设置已保存。';
  if (view.status === 'pending') return '设置已保存，正在等待播放器应用…';
  if (view.settings.master_bypassed) return '均衡器和低高音增强已暂停，设置已保留';
  const tone = activeTone(view.settings);
  if (!view.settings.enabled && !tone) return '均衡器已关闭';
  if (!tone && view.settings.gains.every((g) => g === 0)) return '当前为平直曲线';
  const toneOnly = tone && !activeGeq(view.settings);
  if (view.status === 'configured')
    return toneOnly ? '低音和高音增强已配置，等待音频处理' : '均衡器已配置，等待音频处理';
  return toneOnly ? '低音和高音增强已应用' : '均衡器已启用';
}

export function equalizerLauncher(view?: EqualizerView) {
  if (!view) return { state: 'loading', label: '正在读取均衡器设置' };
  if (view.damaged || view.status === 'failed')
    return { state: 'problem', label: equalizerMessage(view) };
  if (view.status === 'pending') return { state: 'pending', label: equalizerMessage(view) };
  if (view.settings.master_bypassed || equalizerOff(view.settings))
    return { state: 'off', label: equalizerMessage(view) };
  const tone = activeTone(view.settings);
  if (!tone && view.settings.gains.every((gain) => gain === 0))
    return { state: 'flat', label: '均衡器已开启，当前为平直曲线' };
  if (view.status === 'unavailable') return { state: 'problem', label: equalizerMessage(view) };
  if (view.status === 'configured') return { state: 'configured', label: equalizerMessage(view) };
  return {
    state: 'processing',
    label: activeGeq(view.settings) ? '均衡器已应用' : equalizerMessage(view),
  };
}

export function effectLauncher(view?: EqualizerView, parametric?: BoundParametricStatus) {
  const equalizer = equalizerLauncher(view);
  if (!parametric) return equalizer;
  const label = {
    configured: '独立参数均衡已配置',
    deferred: '独立参数均衡等待音频加载',
    detach_required: '独立参数均衡需要清理',
  }[parametric];
  return {
    state:
      parametric === 'detach_required' || equalizer.state === 'problem'
        ? 'problem'
        : equalizer.state === 'processing'
          ? 'processing'
          : parametric === 'deferred'
            ? 'pending'
            : 'configured',
    label:
      equalizer.state === 'off' || equalizer.state === 'loading'
        ? label
        : label + '；' + equalizer.label,
  };
}
