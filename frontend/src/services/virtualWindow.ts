export interface RowWindow {
  start: number;
  end: number;
}

/** Keep a buffer on both sides, and move it in chunks rather than on every pixel. */
export function rowWindow(
  count: number,
  rowHeight: number,
  viewport: number,
  scrollTop: number,
  previous?: RowWindow,
  buffer = 8,
): RowWindow {
  const first = Math.min(count, Math.floor(Math.max(0, scrollTop) / rowHeight));
  const last = Math.min(count, Math.ceil((Math.max(0, scrollTop) + viewport) / rowHeight));
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
