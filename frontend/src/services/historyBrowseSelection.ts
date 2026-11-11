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

/** The table still has a group when a modifier opens an unrelated row. */
export function historyBrowseTableGroup(selection: HistoryBrowseSelection): string[] {
  return selection.ids.length > 1 ? [...selection.ids] : [];
}

/** Do not offer a destructive group action on a row outside that group. */
export function historyBrowseRemovalIds(
  clickedId: string,
  tableGroup: readonly string[],
): string[] {
  if (!tableGroup.length) return [clickedId];
  return tableGroup.includes(clickedId) ? [...tableGroup] : [];
}
import type { HistoryPage, HistoryQueueKind, HistoryRowMenuTarget } from './history';

/** Resolve display labels without converting a browse gesture into batch order. */
export function historyBrowseVoiceDownloadTracks(page: HistoryPage, target: HistoryRowMenuTarget) {
  if (
    page.kind !== 'voice' ||
    target.history_kind !== 'voice' ||
    page.revision !== target.revision ||
    target.selected_ids.length === 1 ||
    new Set(target.selected_ids).size !== target.selected_ids.length
  )
    return;
  const voices = new Map(
    page.entries.flatMap(({ id, resource }) =>
      resource.kind === 'voice' && resource.voice.id === id ? [[id, resource.voice.name]] : [],
    ),
  );
  if (!voices.has(target.clicked_id)) return;
  // M225 keeps selectedData in the table, even if a modifier opened another row.
  const ids = target.selected_ids.length ? target.selected_ids : [target.clicked_id];
  return ids.every((id) => voices.has(id))
    ? ids.map((id) => ({ id, name: voices.get(id)! }))
    : undefined;
}

/** M556 disables the clicked song, or a group only if every selected song is blocked. */
export function historyBrowseSongMenuBlocked(
  kind: HistoryQueueKind,
  clickedId: string,
  selectedIds: readonly string[],
  blockedIds: ReadonlySet<string>,
): boolean {
  if (kind !== 'song') return false;
  return selectedIds.length
    ? selectedIds.every((id) => blockedIds.has(id))
    : blockedIds.has(clickedId);
}
