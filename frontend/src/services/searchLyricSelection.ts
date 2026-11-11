// M106 selects by Song identity, but a range endpoint retains its actual lyric row.
export interface LyricSelectionAnchor {
  index: number;
  id: string;
}
export interface LyricSelection {
  ids: ReadonlySet<string>;
  anchor?: LyricSelectionAnchor;
}
export interface LyricSelectionGesture {
  button: number;
  ctrl: boolean;
  shift: boolean;
}

export const lyricResultKey = (index: number, id: string) => `lyric:${index}:${id}`;

export function emptyLyricSelection(): LyricSelection {
  return { ids: new Set() };
}

export function lyricSelectionOnly(
  selection: LyricSelection,
  row: LyricSelectionAnchor,
  setAnchor = true,
): LyricSelection {
  const anchor = setAnchor ? { index: row.index, id: row.id } : selection.anchor;
  if (
    selection.ids.size === 1 &&
    selection.ids.has(row.id) &&
    anchor?.index === selection.anchor?.index &&
    anchor?.id === selection.anchor?.id
  )
    return selection;
  return { ids: new Set([row.id]), anchor };
}

export function lyricSelectionDown(
  selection: LyricSelection,
  displayed: readonly string[],
  row: LyricSelectionAnchor,
  { button, ctrl, shift }: LyricSelectionGesture,
): { selection: LyricSelection; collapse: boolean } {
  if (displayed[row.index] !== row.id || (button !== 0 && button !== 2))
    return { selection, collapse: false };
  if (button === 2) {
    const ordinary = !ctrl && !shift;
    return {
      selection:
        ordinary && !selection.ids.has(row.id) ? lyricSelectionOnly(selection, row) : selection,
      collapse: false,
    };
  }
  const anchor = selection.anchor;
  if (shift && anchor && displayed[anchor.index] === anchor.id) {
    const range = displayed.slice(
      Math.min(anchor.index, row.index),
      Math.max(anchor.index, row.index) + 1,
    );
    if (!ctrl) return { selection: { ids: new Set(range), anchor }, collapse: false };
    const ids = new Set(selection.ids);
    for (const id of range) ids.delete(id);
    return { selection: { ids, anchor }, collapse: false };
  }
  if (ctrl) {
    const ids = new Set(selection.ids);
    if (ids.has(row.id)) ids.delete(row.id);
    else ids.add(row.id);
    return { selection: { ids, anchor: { index: row.index, id: row.id } }, collapse: false };
  }
  if (shift) return { selection, collapse: false };
  if (selection.ids.has(row.id)) return { selection, collapse: true };
  return { selection: lyricSelectionOnly(selection, row), collapse: false };
}

export function lyricSelectionUp(
  selection: LyricSelection,
  row: LyricSelectionAnchor,
  { button, ctrl, shift }: LyricSelectionGesture,
  collapse: boolean,
): LyricSelection {
  return collapse && button === 0 && !ctrl && !shift
    ? lyricSelectionOnly(selection, row)
    : selection;
}

export function lyricSelectionAll(
  selection: LyricSelection,
  displayed: readonly string[],
): LyricSelection {
  return { ids: new Set(displayed), anchor: selection.anchor };
}

export function reconcileLyricSelection(
  selection: LyricSelection,
  displayed: readonly string[],
): LyricSelection {
  const available = new Set(displayed);
  const ids = new Set([...selection.ids].filter((id) => available.has(id)));
  const anchor =
    selection.anchor && displayed[selection.anchor.index] === selection.anchor.id
      ? selection.anchor
      : undefined;
  return ids.size === selection.ids.size && anchor === selection.anchor
    ? selection
    : { ids, anchor };
}

// Other selected Songs take their first displayed occurrence (M225); the clicked
// Song's one entry uses its actual hit receipt instead of the first duplicate.
export function lyricGroupIndexes(
  displayed: readonly string[],
  selected: ReadonlySet<string>,
  clicked: number,
  scope: 'playback' | 'destination' = 'playback',
): number[] | undefined {
  const id = displayed[clicked];
  if (!id || selected.size < 2 || (scope === 'playback' && !selected.has(id))) return;
  const first = new Map<string, number>();
  displayed.forEach((value, index) => {
    if (!first.has(value)) first.set(value, index);
  });
  if (selected.has(id)) first.set(id, clicked);
  const indexes: number[] = [];
  for (const value of selected) {
    const index = first.get(value);
    if (index === undefined) return;
    indexes.push(index);
  }
  return indexes;
}
