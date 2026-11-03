export interface LyricAnchor {
  index: number;
  offset: number;
}

export function lyricOffsets(heights: readonly number[]) {
  const offsets = [0];
  for (const height of heights) offsets.push(offsets[offsets.length - 1]! + height);
  return offsets;
}

export function lyricAnchor(offsets: readonly number[], position: number): LyricAnchor | undefined {
  const last = offsets.length - 1;
  if (last < 1 || position >= offsets[last]!) return undefined;
  let low = 0;
  let high = last;
  while (high - low > 1) {
    const middle = (low + high) >> 1;
    if (offsets[middle]! <= position) low = middle;
    else high = middle;
  }
  return { index: low, offset: position - offsets[low]! };
}

export function lyricRange(offsets: readonly number[], top: number, height: number, overscan = 5) {
  const length = offsets.length - 1;
  if (!length) return { start: 0, end: 0 };
  const first = lyricAnchor(offsets, top)?.index ?? length - 1;
  const last = lyricAnchor(offsets, top + height)?.index ?? length - 1;
  return { start: Math.max(0, first - overscan), end: Math.min(length, last + overscan + 1) };
}
