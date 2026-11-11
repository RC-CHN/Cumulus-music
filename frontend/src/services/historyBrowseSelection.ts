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
import type {
  HistoryCollectRowTarget,
  HistoryPage,
  HistoryQueueKind,
  HistoryRowMenuTarget,
  HistorySongRowDownloadTarget,
} from './history';

function historyBrowseRows(
  page: HistoryPage,
  kind: HistoryQueueKind,
  target: Omit<HistoryRowMenuTarget, 'history_kind'>,
) {
  const { revision, clicked_id: clicked, selected_ids: group } = target;
  if (page.kind !== kind || page.revision !== revision || group.length === 1) return;
  const byId = new Map(
    page.entries.filter(({ resource }) => resource.kind === kind).map((row) => [row.id, row]),
  );
  const clickedRow = byId.get(clicked);
  if (!clickedRow) return;
  // The PC table keeps selectedData even when a modifier opens a different row.
  const ids = group.length ? group : [clicked];
  const seen = new Set<string>();
  const rows = [];
  for (const id of ids) {
    const row = byId.get(id);
    if (!row || seen.has(id)) return;
    seen.add(id);
    rows.push(row);
  }
  return { clicked: clickedRow, rows };
}

export function historyBrowseSongCollectionRows(
  page: HistoryPage,
  target: HistoryCollectRowTarget,
) {
  return historyBrowseRows(page, 'song', target)?.rows;
}

/** Freeze only the native history IDs, without changing the clicked row or gesture order. */
export function historyBrowseSongDownloadTarget(
  page: HistoryPage,
  target: HistoryRowMenuTarget,
): HistorySongRowDownloadTarget | undefined {
  if (target.history_kind !== 'song' || !historyBrowseRows(page, 'song', target)) return;
  return {
    history_kind: 'song',
    revision: target.revision,
    clicked_id: target.clicked_id,
    selected_ids: [...target.selected_ids],
  };
}

/** Resolve display labels without converting a browse gesture into batch order. */
export function historyBrowseVoiceDownloadTracks(page: HistoryPage, target: HistoryRowMenuTarget) {
  if (target.history_kind !== 'voice') return;
  const resolved = historyBrowseRows(page, 'voice', target);
  if (!resolved) return;
  const {
    rows,
    clicked: { resource: clicked },
  } = resolved;
  if (clicked.kind !== 'voice' || clicked.voice.id !== target.clicked_id) return;
  const tracks = rows.flatMap(({ id, resource }) =>
    resource.kind === 'voice' && resource.voice.id === id
      ? [{ id, name: resource.voice.name }]
      : [],
  );
  if (tracks.length === rows.length) return tracks;
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
