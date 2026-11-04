export const searchPlaylistColumns = [
  { id: 'num', name: '#', label: '序号', width: 45, grow: 0 },
  { id: 'title', name: '标题', width: 200, grow: 4, resizer: 0 },
  { id: 'trackCount', name: '歌曲数', width: 79, grow: 1, resizer: 1 },
  { id: 'creator', name: '创建者', width: 128, grow: 1, resizer: 2 },
  { id: 'playCount', name: '播放量', width: 92, grow: 0 },
] as const;
export type PlaylistColumnHandle = 0 | 1 | 2;
type Shares = readonly [number, number, number, number];

export const playlistBaseWidth = searchPlaylistColumns.reduce((sum, { width }) => sum + width, 0);
export const initialPlaylistShares: Shares = [4 / 6, 1 / 6, 1 / 6, 0];
const columnStorageKey = `cumulus.search.column-grow.v1:${searchPlaylistColumns
  .map(({ id, grow, width }) => `${id}-${grow}-${width}`)
  .join(':')}`;

function validShares(value: unknown): value is Shares {
  return (
    Array.isArray(value) &&
    value.length === 4 &&
    value.every((share) => typeof share === 'number' && share >= 0 && share <= 1) &&
    Math.abs(value.reduce((sum, share) => sum + share, 0) - 1) < 1e-8
  );
}

export function readPlaylistColumnShares(storage?: Pick<Storage, 'getItem'>): Shares {
  try {
    const value: unknown = JSON.parse(
      (storage ?? localStorage).getItem(columnStorageKey) ?? 'null',
    );
    if (validShares(value)) return value;
  } catch {
    // Invalid or unavailable WebView preferences leave the default usable.
  }
  return initialPlaylistShares;
}

export function writePlaylistColumnShares(shares: Shares, storage?: Pick<Storage, 'setItem'>) {
  try {
    (storage ?? localStorage).setItem(columnStorageKey, JSON.stringify(shares));
  } catch {
    // Keep the live table usable if local storage cannot write.
  }
}

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
  return next.every((share, index) => share === shares[index]) ? shares : next;
}
