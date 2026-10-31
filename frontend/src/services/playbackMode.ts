import type { PlaybackMode } from './player';
export type ModeFamily = 'ordinary' | 'fm' | 'together';
// M/7 nextPlayingMode: ordinary (heartbeat unavailable) and together rings.
// FM's two-state switch comes from PlayMode.tsx and fmPlaying, independently.
const ordinary: PlaybackMode[] = ['sequential', 'order_loop', 'single_loop', 'random'];
const rings: Record<ModeFamily, PlaybackMode[]> = {
  ordinary,
  together: ['order_loop', 'single_loop', 'random'],
  fm: ['sequential', 'single_loop'],
};
export function nextMode(current: PlaybackMode, family: ModeFamily): PlaybackMode {
  const ring = rings[family];
  return ring[(Math.max(ring.indexOf(current), 0) + 1) % ring.length]!;
}
export function modeLabel(current: PlaybackMode, family: ModeFamily) {
  if (current === 'sequential' && family === 'fm') return '无限漫游';
  return {
    order_loop: '列表循环',
    single_loop: '单曲循环',
    random: '随机播放',
    sequential: '顺序播放',
  }[current];
}
export function modeIcon(current: PlaybackMode, family: ModeFamily) {
  if (current === 'sequential' && family === 'fm') return 'repeat';
  return { order_loop: 'repeat', single_loop: 'repeatOne', random: 'shuffle', sequential: 'list' }[
    current
  ];
}
