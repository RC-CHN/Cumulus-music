type Column = {
  id: string;
  name: string;
  label?: string;
  width: number;
  grow: number;
  resizable?: boolean;
};
type Shares = readonly number[];

function tableLayout(name: string, columns: readonly Column[]) {
  const eligible = columns.filter((column) => column.resizable);
  const grow = eligible.reduce((sum, column) => sum + column.grow, 0);
  const shares: Shares = eligible.map((column) => column.grow / grow);
  const baseWidth = columns.reduce((sum, column) => sum + column.width, 0);
  // Preserve the existing playlist preference and isolate different official layouts.
  const signature = columns.map(({ id, grow, width }) => `${id}-${grow}-${width}`).join(':');
  return {
    name,
    columns: columns.map((column) => ({
      ...column,
      share: column.resizable ? eligible.indexOf(column) : undefined,
    })),
    shares,
    baseWidth,
    storageKey: `cumulus.search.column-grow.v1:${signature}`,
  };
}

export const searchTableLayouts = {
  playlist: tableLayout('搜索歌单结果', [
    { id: 'num', name: '#', label: '序号', width: 45, grow: 0 },
    { id: 'title', name: '标题', width: 200, grow: 4, resizable: true },
    { id: 'trackCount', name: '歌曲数', width: 79, grow: 1, resizable: true },
    { id: 'creator', name: '创建者', width: 128, grow: 1, resizable: true },
    { id: 'playCount', name: '播放量', width: 92, grow: 0, resizable: true },
  ]),
  album: tableLayout('搜索专辑结果', [
    { id: 'num', name: '#', label: '序号', width: 45, grow: 0 },
    { id: 'title', name: '标题', width: 200, grow: 4, resizable: true },
    { id: 'artists', name: '作者', width: 128, grow: 2, resizable: true },
    { id: 'publishTime', name: '时间', width: 128, grow: 2 },
  ]),
  voice: tableLayout('搜索声音结果', [
    { id: 'num', name: '#', label: '序号', width: 45, grow: 0 },
    { id: 'title', name: '标题', width: 200, grow: 5, resizable: true },
    { id: 'listenerCount', name: '播放量', width: 79, grow: 0, resizable: true },
    { id: 'duration', name: '时长', width: 64, grow: 0, resizable: true },
  ]),
};
export type SearchTableKind = keyof typeof searchTableLayouts;

function validShares(value: unknown, length: number): value is Shares {
  return (
    Array.isArray(value) &&
    value.length === length &&
    value.every((share) => typeof share === 'number' && share >= 0 && share <= 1) &&
    Math.abs(value.reduce((sum, share) => sum + share, 0) - 1) < 1e-8
  );
}

export function readSearchColumnShares(
  kind: SearchTableKind,
  storage?: Pick<Storage, 'getItem'>,
): Shares {
  const layout = searchTableLayouts[kind];
  try {
    const value: unknown = JSON.parse(
      (storage ?? localStorage).getItem(layout.storageKey) ?? 'null',
    );
    if (validShares(value, layout.shares.length)) return value;
  } catch {
    // Invalid or unavailable WebView preferences leave the default usable.
  }
  return layout.shares;
}

export function writeSearchColumnShares(
  kind: SearchTableKind,
  shares: Shares,
  storage?: Pick<Storage, 'setItem'>,
) {
  try {
    (storage ?? localStorage).setItem(searchTableLayouts[kind].storageKey, JSON.stringify(shares));
  } catch {
    // Keep the live table usable if local storage cannot write.
  }
}

export function resizeSearchColumns(
  shares: Shares,
  index: number,
  pixels: number,
  surplus: number,
): Shares {
  if (surplus <= 0 || !pixels || index < 0 || index >= shares.length - 1) return shares;
  const next = [...shares];
  let change = pixels / surplus;
  if (change < 0) {
    change = Math.max(change, -next[index]!);
    next[index]! += change;
    next[index + 1]! -= change;
  } else {
    for (let right = index + 1; right < next.length && change > 0; right++) {
      const transfer = Math.min(change, next[right]!);
      next[index]! += transfer;
      next[right]! -= transfer;
      change -= transfer;
    }
  }
  return next.every((share, i) => share === shares[i]) ? shares : next;
}
