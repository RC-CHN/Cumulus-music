/** Recovered 106.useResizer: only redistribute surplus between resizable columns. */
export interface TableColumn {
  width: number;
  grow: number;
  resize: boolean;
}
export const minimumTableWidth = (columns: readonly TableColumn[]) =>
  columns.reduce((sum, column) => sum + column.width, 0);
export function tableColumnWidths(
  width: number,
  columns: readonly TableColumn[],
  growth = columns.map((column) => column.grow),
): number[] {
  const extra = Math.max(0, width - minimumTableWidth(columns));
  const total = growth.reduce((a, b) => a + b, 0);
  return columns.map((column, i) => column.width + (total ? (extra * growth[i]!) / total : 0));
}
export function resizeTableColumns(
  columns: readonly TableColumn[],
  growth: number[],
  column: number,
  pixels: number,
  width: number,
): number[] {
  const extra = width - minimumTableWidth(columns);
  if (extra <= 0 || !Number.isFinite(pixels) || !columns[column]?.resize) return growth;
  const following = columns.map((c, i) => (i > column && c.resize ? i : -1)).filter((i) => i >= 0);
  if (!following.length) return growth;
  const next = [...growth];
  const delta = (pixels / extra) * growth.reduce((a, b) => a + b, 0);
  if (delta > 0) {
    let remaining = delta;
    for (const i of following) {
      const transfer = Math.min(remaining, next[i]!);
      next[i]! -= transfer;
      next[column]! += transfer;
      remaining -= transfer;
    }
  } else {
    const transfer = Math.min(-delta, next[column]!);
    next[column]! -= transfer;
    next[following[0]!]! += transfer;
  }
  return next;
}
