/** The PC track viewer stores selected identities in an ordered Set (M/106). */
export type PlaylistDragSelection = Readonly<{ ids: ReadonlySet<string>; anchor?: string }>;
export const emptyPlaylistDragSelection = (): PlaylistDragSelection => ({ ids: new Set() });

export function playlistRowDown(
  state: PlaylistDragSelection,
  displayed: readonly string[],
  id: string,
  toggle: boolean,
  range: boolean,
  secondary: boolean,
): PlaylistDragSelection {
  if (!displayed.includes(id)) return state;
  const anchor = state.anchor ? displayed.indexOf(state.anchor) : -1;
  if (secondary) return state.ids.has(id) ? state : { ids: new Set([id]), anchor: id };
  if (range && anchor >= 0) {
    const index = displayed.indexOf(id);
    const rows = displayed.slice(Math.min(anchor, index), Math.max(anchor, index) + 1);
    if (!toggle) return { ids: new Set(rows), anchor: state.anchor };
    const ids = new Set(state.ids);
    for (const row of rows) ids.delete(row);
    return { ids, anchor: state.anchor };
  }
  if (toggle) {
    const ids = new Set(state.ids);
    if (ids.has(id)) ids.delete(id);
    else ids.add(id);
    return { ids, anchor: id };
  }
  if (range || state.ids.has(id)) return state;
  return { ids: new Set([id]), anchor: id };
}

export function playlistRowRelease(id: string): PlaylistDragSelection {
  return { ids: new Set([id]), anchor: id };
}

export function playlistSelectionNeighbor(
  displayed: readonly string[],
  selected: ReadonlySet<string>,
  direction: -1 | 1,
) {
  const start = direction === -1 ? 0 : displayed.length - 1;
  for (let index = start; index >= 0 && index < displayed.length; index -= direction) {
    if (selected.has(displayed[index])) {
      return Math.max(0, Math.min(displayed.length - 1, index + direction));
    }
  }
}
