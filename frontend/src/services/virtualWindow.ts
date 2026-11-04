export interface RowWindow {
  start: number;
  end: number;
}

/** A single taller row, for a section break inside a virtual list. */
export interface RowExpansion {
  index: number;
  extra: number;
}

export function rowTop(index: number, rowHeight: number, expansion?: RowExpansion) {
  return index * rowHeight + (expansion && index > expansion.index ? expansion.extra : 0);
}

export function rowExtent(index: number, rowHeight: number, expansion?: RowExpansion) {
  return rowHeight + (expansion?.index === index ? expansion.extra : 0);
}

function rowPosition(offset: number, rowHeight: number, expansion?: RowExpansion) {
  const position = Math.max(0, offset);
  if (!expansion) return position / rowHeight;
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
  expansion?: RowExpansion,
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
  expansion?: RowExpansion,
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
  expansion?: RowExpansion,
) {
  if (!Number.isInteger(index) || index < 0 || index >= count || height <= 0 || viewport <= 0)
    return null;
  const center = rowTop(index, height, expansion) + rowExtent(index, height, expansion) / 2;
  return Math.max(0, Math.min(center - viewport / 2, rowTop(count, height, expansion) - viewport));
}
