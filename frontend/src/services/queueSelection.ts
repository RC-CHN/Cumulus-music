/** Official table selection (M106): defer collapsing a selected group until
 * pointer-up, so dragging any selected row moves the entire selection. */
export interface QueueSelection {
  selected: ReadonlySet<string>;
  anchor?: string;
}

export function selectQueueRows(
  ids: readonly string[],
  state: QueueSelection,
  id: string,
  toggle = false,
  range = false,
): QueueSelection {
  const index = ids.indexOf(id),
    anchor = state.anchor ? ids.indexOf(state.anchor) : -1;
  if (index < 0) return state;
  if (range && anchor >= 0) {
    const selected = toggle ? new Set(state.selected) : new Set<string>();
    for (const item of ids.slice(Math.min(anchor, index), Math.max(anchor, index) + 1)) {
      if (toggle) selected.delete(item);
      else selected.add(item);
    }
    return { selected, anchor: state.anchor };
  }
  if (toggle) {
    const selected = new Set(state.selected);
    if (selected.has(id)) selected.delete(id);
    else selected.add(id);
    return { selected, anchor: id };
  }
  return { selected: new Set([id]), anchor: id };
}

/** Insert the selected block at a boundary in the original displayed order.
 * Identity is entryId, never trackId: the same recording may appear twice. */
export function reorderQueueRows(
  ids: readonly string[],
  selected: ReadonlySet<string>,
  boundary: number,
  keepSameOrder = false,
): string[] | undefined {
  if (
    !selected.size ||
    !Number.isInteger(boundary) ||
    boundary < 0 ||
    boundary > ids.length ||
    new Set(ids).size !== ids.length ||
    [...selected].some((id) => !ids.includes(id))
  )
    return;
  const moving = ids.filter((id) => selected.has(id));
  const rest = ids.filter((id) => !selected.has(id));
  const insert = ids.slice(0, boundary).filter((id) => !selected.has(id)).length;
  rest.splice(insert, 0, ...moving);
  return keepSameOrder || rest.some((id, index) => id !== ids[index]) ? rest : undefined;
}

/** Keep the original insertion offset and exact selected IDs distinct from the
 * resulting permutation. A divider drop can change only the selected marker. */
export function displayedQueueMove(
  rows: readonly { entryId: string; moreRecommendation?: boolean }[],
  selected: ReadonlySet<string>,
  offset: number,
  separatorBefore?: string | null,
) {
  const ids = rows.map((row) => row.entryId);
  const divider = separatorBefore ? ids.indexOf(separatorBefore) : -1;
  const possibleMarkerChange =
    divider >= 0
      ? selected.has(ids[divider]!) ||
        (offset <= divider &&
          rows.some((row) => row.moreRecommendation === true && selected.has(row.entryId)))
      : rows.some((row) => row.moreRecommendation === true);
  const entry_ids = reorderQueueRows(ids, selected, offset, possibleMarkerChange);
  if (!entry_ids) return;
  return {
    kind: 'move' as const,
    entry_ids,
    movement: { entry_ids: ids.filter((id) => selected.has(id)), offset },
  };
}
