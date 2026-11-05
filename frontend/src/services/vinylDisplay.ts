import type { LyricLine } from './player';

export const vinylDisplayModes = [
  { value: 'classic', label: '标准' },
  { value: 'cover', label: '黑胶' },
  { value: 'lyric', label: '歌词' },
] as const;
export type VinylDisplayMode = (typeof vinylDisplayModes)[number]['value'];
const key = 'cumulus.playback.vinyl-display-mode.v1';

export function readVinylDisplayMode(storage?: Pick<Storage, 'getItem'>): VinylDisplayMode {
  try {
    const value = (storage ?? localStorage).getItem(key);
    return vinylDisplayModes.find((mode) => mode.value === value)?.value ?? 'classic';
  } catch {
    return 'classic';
  }
}

export function storeVinylDisplayMode(value: VinylDisplayMode, storage?: Pick<Storage, 'setItem'>) {
  try {
    (storage ?? localStorage).setItem(key, value);
  } catch {
    // A denied preference write does not reset the live device selection.
  }
}

// PC M185 uses a 5ms look-ahead and shows the first line before its timestamp.
export function vinylCoverLyric(lines: readonly LyricLine[], position: number): string {
  const target = position + 0.005;
  let lo = 0;
  let hi = lines.length - 1;
  while (lo <= hi) {
    const middle = Math.floor((lo + hi) / 2);
    const timestamp = lines[middle]?.time;
    if (timestamp === target) return lines[middle]?.lyric ?? '';
    if (timestamp !== undefined && timestamp < target) lo = middle + 1;
    else hi = middle - 1;
  }
  return lines[Math.max(0, lo - 1)]?.lyric ?? '';
}
