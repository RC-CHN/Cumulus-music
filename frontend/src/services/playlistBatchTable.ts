/** Official 352.Ze widths/growth and 106.useResizer distribution. */
export type BatchGrowth = [number, number, number];
export type BatchResizeColumn = 0 | 1;
export const batchDefaultGrowth: BatchGrowth = [360, 230, 30];
export const batchMinimumWidth = 464;
export function batchColumnWidths(width: number, growth: BatchGrowth): number[] {
  const extra = Math.max(0, width - batchMinimumWidth);
  const total = growth.reduce((a, b) => a + b, 0);
  return [
    45,
    180 + (extra * growth[0]) / total,
    115 + (extra * growth[1]) / total,
    60,
    64 + (extra * growth[2]) / total,
  ];
}
export function resizeBatchColumns(
  growth: BatchGrowth,
  column: BatchResizeColumn,
  pixels: number,
  width: number,
): BatchGrowth {
  const extra = width - batchMinimumWidth;
  if (extra <= 0 || !Number.isFinite(pixels)) return growth;
  const next: BatchGrowth = [...growth];
  const delta = (pixels / extra) * growth.reduce((a, b) => a + b, 0);
  if (delta > 0) {
    let remaining = delta;
    for (let i = column + 1; i < next.length; i++) {
      const transfer = Math.min(remaining, next[i]!);
      next[i]! -= transfer;
      next[column] += transfer;
      remaining -= transfer;
    }
  } else {
    const transfer = Math.min(-delta, next[column]);
    next[column] -= transfer;
    next[column + 1]! += transfer;
  }
  return next;
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
