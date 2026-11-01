import type { TableColumn } from './tableColumns.ts';
// UploadedTrackViewer bases/growth; the first checkbox is a community addition.
export const cloudSongColumns: readonly TableColumn[] = [
  { width: 28, grow: 0, resize: false },
  { width: 45, grow: 0, resize: false },
  { width: 180, grow: 360, resize: true },
  { width: 115, grow: 230, resize: true },
  { width: 64, grow: 0, resize: false },
  { width: 90, grow: 45, resize: true },
  { width: 60, grow: 30, resize: true },
];
export function cloudSongSelection(
  ids: string[],
  selected: Set<string>,
  index: number,
  anchor: string | undefined,
  modifier: { ctrl: boolean; shift: boolean },
): { selected: Set<string>; anchor: string | undefined } {
  const id = ids[index];
  if (!id) return { selected, anchor };
  if (modifier.shift && anchor && ids.includes(anchor)) {
    const from = ids.indexOf(anchor),
      range = ids.slice(Math.min(from, index), Math.max(from, index) + 1);
    if (modifier.ctrl)
      return { selected: new Set([...selected].filter((key) => !range.includes(key))), anchor };
    return { selected: new Set(range), anchor };
  }
  const next = modifier.ctrl ? new Set(selected) : new Set<string>();
  if (modifier.ctrl && next.has(id)) next.delete(id);
  else next.add(id);
  return { selected: next, anchor: id };
}
/** Capacity.tsx default branch uses binary G/TB and a visible nonzero minimum. */
export function cloudCapacityText(bytes: number | null | undefined): string {
  if (bytes == null || !Number.isFinite(bytes) || bytes < 0) return '—';
  const gib = bytes / 1024 ** 3;
  return gib === 0
    ? '0G'
    : gib < 1
      ? `${gib.toFixed(1)}G`
      : gib < 1024
        ? `${Math.round(gib)}G`
        : `${(gib / 1024).toFixed(1)}TB`;
}
export function cloudCapacityPercent(
  size: number | null,
  maximum: number | null,
): number | undefined {
  if (size == null || maximum == null || maximum <= 0) return;
  return size === 0 ? 0 : Math.min(100, Math.max(size / maximum, 0.001) * 100);
}
