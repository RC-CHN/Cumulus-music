export interface RowWindow {
  start: number;
  end: number;
}

/** A single taller row, for a section break inside a virtual list. */
export interface RowExpansion {
  index: number;
  extra: number;
}

export type RowLayout = RowExpansion | { extraBefore: readonly number[] };

/** Prefix sums keep arbitrary expanded rows cheap to locate by scroll position. */
export function expandedRows(extras: readonly number[]): RowLayout | undefined {
  let total = 0;
  const extraBefore = [0];
  for (const extra of extras) extraBefore.push((total += extra));
  return total > 0 ? { extraBefore } : undefined;
}

export function rowTop(index: number, rowHeight: number, expansion?: RowLayout) {
  const extra = !expansion
    ? 0
    : 'extraBefore' in expansion
      ? (expansion.extraBefore[index] ?? expansion.extraBefore.at(-1) ?? 0)
      : index > expansion.index
        ? expansion.extra
        : 0;
  return index * rowHeight + extra;
}

export function rowExtent(index: number, rowHeight: number, expansion?: RowLayout) {
  return rowTop(index + 1, rowHeight, expansion) - rowTop(index, rowHeight, expansion);
}

function rowPosition(offset: number, rowHeight: number, expansion?: RowLayout) {
  const position = Math.max(0, offset);
  if (!expansion) return position / rowHeight;
  if ('extraBefore' in expansion) {
    let lo = 0;
    let hi = expansion.extraBefore.length;
    while (lo + 1 < hi) {
      const mid = Math.floor((lo + hi) / 2);
      if (rowTop(mid, rowHeight, expansion) <= position) lo = mid;
      else hi = mid;
    }
    return lo + (position - rowTop(lo, rowHeight, expansion)) / rowExtent(lo, rowHeight, expansion);
  }
  const top = expansion.index * rowHeight;
  if (position <= top) return position / rowHeight;
  const height = rowHeight + expansion.extra;
  if (position < top + height) return expansion.index + (position - top) / height;
  return (position - expansion.extra) / rowHeight;
}

/** Nearest insertion boundary in display order, including a section break. */
export function rowBoundary(
  offset: number,
  count: number,
  rowHeight: number,
  expansion?: RowLayout,
) {
  return Math.min(count, Math.floor(rowPosition(offset, rowHeight, expansion) + 0.5));
}

/** Keep a buffer on both sides, and move it in chunks rather than on every pixel. */
export function rowWindow(
  count: number,
  rowHeight: number,
  viewport: number,
  scrollTop: number,
  previous?: RowWindow,
  buffer = 8,
  expansion?: RowLayout,
): RowWindow {
  const first = Math.min(count, Math.floor(rowPosition(scrollTop, rowHeight, expansion)));
  const last = Math.min(
    count,
    Math.ceil(rowPosition(Math.max(0, scrollTop) + viewport, rowHeight, expansion)),
  );
  const guard = Math.min(3, buffer);
  if (
    previous &&
    previous.end <= count &&
    previous.start <= first &&
    previous.end >= last &&
    (previous.start === 0 || first - previous.start >= guard) &&
    (previous.end === count || previous.end - last >= guard)
  )
    return previous;
  return { start: Math.max(0, first - buffer), end: Math.min(count, last + buffer) };
}

/** Center a row when possible, without scrolling past either edge. */
export function centeredRowTop(
  index: number,
  count: number,
  height: number,
  viewport: number,
  expansion?: RowLayout,
) {
  if (!Number.isInteger(index) || index < 0 || index >= count || height <= 0 || viewport <= 0)
    return null;
  const center = rowTop(index, height, expansion) + rowExtent(index, height, expansion) / 2;
  return Math.max(0, Math.min(center - viewport / 2, rowTop(count, height, expansion) - viewport));
}
