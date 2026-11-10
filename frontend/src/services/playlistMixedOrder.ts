import type { TrackPage, TrackView } from './contracts';
import type { PlaylistMixedOrderRow } from './playlists';

function typedRow(
  track: TrackView,
  rows: Record<string, string>,
): PlaylistMixedOrderRow | undefined {
  const id = track.id;
  if (track.local) {
    const revision = Object.hasOwn(rows, id) ? rows[id] : undefined;
    return revision ? { kind: 'local', id, revision } : undefined;
  }
  if (/^(?=[0-9]*[1-9])[0-9]+$/.test(id)) return { kind: 'online', id };
}

/** M/195 supplies selectedData in Set order for multiple rows, otherwise
 * M/3085 uses the dragged row. Keep missing but still bound rows distinct
 * from unbound legacy identities. */
export function mixedDraggedRows(
  page: TrackPage,
  displayed: ReadonlyMap<string, TrackView>,
  selected: ReadonlySet<string>,
  dragged: string,
): PlaylistMixedOrderRow[] | undefined {
  const local = page.playlistLocal;
  if (
    !page.playlist?.owned ||
    page.playlist.favorite ||
    !local?.hasMembers ||
    local.hasUnboundLegacy
  )
    return;
  if (!displayed.has(dragged)) return;
  const rows: PlaylistMixedOrderRow[] = [];
  for (const id of selected.size > 1 ? selected : [dragged]) {
    const track = displayed.get(id);
    if (!track) return;
    const row = typedRow(track, local.rows);
    if (!row) return;
    rows.push(row);
  }
  return rows;
}

/** A group is selected by displayed IDs; reject it if an ID is ambiguous
 * even when only one of the colliding rows is visible under a filter. A
 * single-row menu has the actual row object and retains the typed identity. */
export function mixedRemovedRows(
  page: TrackPage,
  selected: readonly TrackView[],
): PlaylistMixedOrderRow[] | undefined {
  const local = page.playlistLocal;
  if (
    !page.playlist?.owned ||
    page.playlist.favorite ||
    !local?.hasMembers ||
    local.hasUnboundLegacy
  )
    return;
  const picked = new Set(selected);
  const ids = new Set(selected.map((track) => track.id));
  if (!picked.size || picked.size !== selected.length || ids.size !== selected.length) return;
  for (const track of page.tracks) {
    if (picked.delete(track)) continue;
    if (selected.length > 1 && ids.has(track.id)) return;
  }
  if (picked.size) return;
  const rows: PlaylistMixedOrderRow[] = [];
  for (const track of selected) {
    const row = typedRow(track, local.rows);
    if (!row) return;
    rows.push(row);
  }
  if (rows.some((row) => row.kind === 'online')) return rows;
}
