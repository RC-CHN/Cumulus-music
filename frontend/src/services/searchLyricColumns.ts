import { readSearchColumnShares, searchTableLayouts } from './searchTableColumns.ts';
import type { TableColumn } from './tableColumns.ts';

const layout = searchTableLayouts.lyrics;

export const searchLyricColumns: readonly TableColumn[] = layout.columns.map(
  ({ width, grow, resizable }) => ({ width, grow, resize: !!resizable }),
);

/** Only presentation ratios leave the table; never persist the keyword, row or owner. */
export function lyricColumnGrowth(storage?: Pick<Storage, 'getItem'>): number[] {
  const shares = readSearchColumnShares('lyrics', storage);
  return layout.columns.map(({ share }) => (share === undefined ? 0 : shares[share]!));
}

export function lyricColumnShares(growth: readonly number[]): number[] | undefined {
  if (
    growth.length !== layout.columns.length ||
    growth.some(
      (value, i) =>
        !Number.isFinite(value) ||
        value < 0 ||
        (layout.columns[i]!.share === undefined && value !== 0),
    )
  )
    return;
  const total = growth.reduce((sum, value) => sum + value, 0);
  if (!Number.isFinite(total) || total <= 0) return;
  return layout.columns.flatMap(({ share }, i) =>
    share === undefined ? [] : [growth[i]! / total],
  );
}
