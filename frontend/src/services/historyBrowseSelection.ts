export interface HistoryBrowseSelection {
  anchor: string | null;
  ids: readonly string[];
}

type Gesture = 'single' | 'toggle' | 'range' | 'subtract-range';

export function selectHistoryBrowseRow(
  selection: HistoryBrowseSelection,
  visibleIds: readonly string[],
  id: string,
  gesture: Gesture,
): HistoryBrowseSelection {
  const at = visibleIds.indexOf(id);
  if (at < 0) return selection;
  if (gesture === 'toggle') {
    const removing = selection.ids.includes(id);
    const ids = removing ? selection.ids.filter((row) => row !== id) : [...selection.ids, id];
    return { anchor: id, ids };
  }
  const anchor = selection.anchor === null ? -1 : visibleIds.indexOf(selection.anchor);
  if (gesture === 'range' || gesture === 'subtract-range') {
    if (anchor < 0) return selection;
    // M106's inclusive span follows display order even when selected upward.
    const span = visibleIds.slice(Math.min(anchor, at), Math.max(anchor, at) + 1);
    if (gesture === 'range') return { anchor: selection.anchor, ids: span };
    const removed = new Set(span);
    return { anchor: selection.anchor, ids: selection.ids.filter((row) => !removed.has(row)) };
  }
  return { anchor: id, ids: [id] };
}

/** The opened row owns a group only if it was part of the user's selection. */
export function historyBrowseMenuIds(
  selection: HistoryBrowseSelection,
  clickedId: string,
): string[] {
  return selection.ids.length > 1 && selection.ids.includes(clickedId) ? [...selection.ids] : [];
}
