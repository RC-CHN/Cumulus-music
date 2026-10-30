import { tableColumnWidths, resizeTableColumns, type TableColumn } from './tableColumns.ts';
/** Official 352.Ze widths/growth and 106.useResizer distribution. */
export type BatchGrowth = [number, number, number];
export type BatchResizeColumn = 0 | 1;
export const batchDefaultGrowth: BatchGrowth = [360, 230, 30];
export const batchMinimumWidth = 464;
export const batchTableColumns: readonly TableColumn[] = [
  { width: 45, grow: 0, resize: false },
  { width: 180, grow: 360, resize: true },
  { width: 115, grow: 230, resize: true },
  { width: 60, grow: 0, resize: false },
  { width: 64, grow: 30, resize: true },
];
export function batchColumnWidths(width: number, growth: BatchGrowth): number[] {
  return tableColumnWidths(width, batchTableColumns, [0, growth[0], growth[1], 0, growth[2]]);
}
export function resizeBatchColumns(
  growth: BatchGrowth,
  column: BatchResizeColumn,
  pixels: number,
  width: number,
): BatchGrowth {
  const input = [0, growth[0], growth[1], 0, growth[2]];
  const next = resizeTableColumns(batchTableColumns, input, column + 1, pixels, width);
  return next === input ? growth : [next[1]!, next[2]!, next[4]!];
}
/** Plain arrows collapse selection from its leading/trailing edge (106.js). */
export function batchSelectionNeighbor(
  ids: string[],
  selected: Set<string>,
  direction: -1 | 1,
): number | undefined {
  if (!ids.length || !selected.size) return;
  if (direction < 0) {
    const first = ids.findIndex((id) => selected.has(id));
    return first < 0 ? undefined : Math.max(0, first - 1);
  }
  for (let i = ids.length - 1; i >= 0; i--)
    if (selected.has(ids[i]!)) return Math.min(ids.length - 1, i + 1);
}
