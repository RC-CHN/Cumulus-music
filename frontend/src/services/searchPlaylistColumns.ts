export const searchPlaylistColumns = [
  { name: '#', label: '序号', width: 45, grow: 0 },
  { name: '标题', width: 200, grow: 4, resizer: 0 },
  { name: '歌曲数', width: 79, grow: 1, resizer: 1 },
  { name: '创建者', width: 128, grow: 1, resizer: 2 },
  { name: '播放量', width: 92, grow: 0 },
] as const;
export type PlaylistColumnHandle = 0 | 1 | 2;
type Shares = readonly [number, number, number, number];

export const playlistBaseWidth = searchPlaylistColumns.reduce((sum, { width }) => sum + width, 0);
export const initialPlaylistShares: Shares = [4 / 6, 1 / 6, 1 / 6, 0];

export function resizePlaylistColumns(
  shares: Shares,
  index: PlaylistColumnHandle,
  pixels: number,
  surplus: number,
): Shares {
  if (surplus <= 0 || !pixels) return shares;
  const next: [number, number, number, number] = [...shares];
  let change = pixels / surplus;
  if (change < 0) {
    change = Math.max(change, -next[index]);
    next[index] += change;
    next[index + 1]! -= change;
  } else {
    for (let right = index + 1; right < next.length && change > 0; right++) {
      const transfer = Math.min(change, next[right]!);
      next[index] += transfer;
      next[right]! -= transfer;
      change -= transfer;
    }
  }
  return next;
}
