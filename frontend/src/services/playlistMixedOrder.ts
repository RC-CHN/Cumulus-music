import type { TrackPage, TrackView } from './contracts';
import type { PlaylistMixedOrderRow } from './playlists';

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
    if (track.local) {
      const revision = Object.hasOwn(local.rows, id) ? local.rows[id] : undefined;
      if (!revision) return;
      rows.push({ kind: 'local', id, revision });
    } else {
      if (!/^(?=[0-9]*[1-9])[0-9]+$/.test(id)) return;
      rows.push({ kind: 'online', id });
    }
  }
  return rows;
}
