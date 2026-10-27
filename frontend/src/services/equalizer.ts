import { equalizerPresets } from '../data/equalizer-presets.ts';
import type { EqualizerView, EqualizerSettings, PlayerAction } from './player.ts';
export { equalizerPresets };
export const equalizerLabels = ['31', '63', '125', '250', '500', '1k', '2k', '4k', '8k', '16k'];
export function equalizerPreset(gains: readonly number[]): number {
  return equalizerPresets.findIndex(
    (p) =>
      p.gains.length === gains.length && p.gains.every((g, i) => Math.abs(g - gains[i]!) < 0.00001),
  );
}
export function equalizerChange(view: EqualizerView, settings: EqualizerSettings): PlayerAction {
  if (
    view.damaged ||
    settings.gains.length !== 10 ||
    settings.gains.some((g) => !Number.isFinite(g) || Math.abs(g) > 12)
  )
    throw new Error('均衡器参数无效');
  return {
    kind: 'equalizer',
    expected_revision: view.revision,
    settings: { enabled: settings.enabled, gains: [...settings.gains] },
  };
}
export function equalizerMessage(view: EqualizerView): string {
  if (view.damaged) return '本地均衡器设置损坏，请修复后重试。';
  if (view.status === 'failed')
    return '设置已保存，但均衡器未能应用。可重新应用，或关闭后继续播放。';
  if (view.status === 'unavailable') return '均衡器组件未就绪，设置已保存。';
  if (view.status === 'pending') return '设置已保存，正在等待播放器应用…';
  if (!view.settings.enabled) return '均衡器已关闭';
  return view.settings.gains.every((g) => g === 0) ? '当前为平直曲线' : '均衡器已启用';
}
