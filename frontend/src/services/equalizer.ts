import { equalizerPresets } from '../data/equalizer-presets.ts';
import type { EqualizerView, EqualizerSettings, EqualizerPreset, PlayerAction } from './player.ts';
export { equalizerPresets };
export const equalizerLabels = ['31', '63', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'];
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
      gains: [...settings.gains],
      ...(tone.bass_db !== 0 ? { bass_db: tone.bass_db } : {}),
      ...(tone.treble_db !== 0 ? { treble_db: tone.treble_db } : {}),
    },
  };
}
export function equalizerMessage(view: EqualizerView): string {
  if (view.damaged) return '本地均衡器设置损坏，请修复后重试。';
  if (view.status === 'failed')
    return '设置已保存，但均衡器未能应用。可重新应用，或关闭后继续播放。';
  if (view.status === 'unavailable') return '均衡器组件未就绪，设置已保存。';
  if (view.status === 'pending') return '设置已保存，正在等待播放器应用…';
  if (!view.settings.enabled) return '均衡器已关闭';
  if (
    view.settings.gains.every((g) => g === 0) &&
    Object.values(equalizerTone(view.settings)).every((g) => g === 0)
  )
    return '当前为平直曲线';
  return view.status === 'configured' ? '均衡器已配置，等待音频处理' : '均衡器已启用';
}

export function equalizerLauncher(view?: EqualizerView) {
  if (!view) return { state: 'loading', label: '正在读取均衡器设置' };
  if (view.damaged) return { state: 'problem', label: equalizerMessage(view) };
  if (!view.settings.enabled) return { state: 'off', label: '均衡器已关闭' };
  if (view.status === 'failed' || view.status === 'unavailable')
    return { state: 'problem', label: equalizerMessage(view) };
  if (view.status === 'pending') return { state: 'pending', label: equalizerMessage(view) };
  if (
    view.settings.gains.every((gain) => gain === 0) &&
    Object.values(equalizerTone(view.settings)).every((gain) => gain === 0)
  )
    return { state: 'flat', label: '均衡器已开启，当前为平直曲线' };
  if (view.status === 'configured') return { state: 'configured', label: equalizerMessage(view) };
  return { state: 'processing', label: '均衡器已应用' };
}
