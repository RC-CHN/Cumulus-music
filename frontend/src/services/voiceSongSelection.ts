// M106:152–223,291–299: selection order is insertion order, not DOM order.
// Card ordinals continue to exist when their rows are outside the render window.
export interface VoiceSongSelection {
  indices: ReadonlySet<number>;
  anchor?: number;
  collapse?: number;
}

function valid(index: number, count: number) {
  return Number.isInteger(index) && index >= 0 && index < count;
}

export function singleVoiceSong(index: number): VoiceSongSelection {
  return { indices: new Set([index]), anchor: index };
}

export function voiceSongDown(
  state: VoiceSongSelection,
  index: number,
  count: number,
  event: { button: number; toggle: boolean; range: boolean },
): VoiceSongSelection {
  if (!valid(index, count)) return state;
  const { button, toggle, range } = event;
  if (button !== 0 && button !== 2) return state;
  const next = { indices: state.indices, anchor: state.anchor };
  if (!toggle && !range) {
    if (!state.indices.has(index)) return singleVoiceSong(index);
    return button === 0 ? { ...next, collapse: index } : next;
  }
  if (button !== 0) return next;
  if (range && state.anchor !== undefined && valid(state.anchor, count)) {
    const indices = toggle ? new Set(state.indices) : new Set<number>();
    for (let row = Math.min(state.anchor, index); row <= Math.max(state.anchor, index); row++) {
      if (toggle) indices.delete(row);
      else indices.add(row);
    }
    return { indices, anchor: state.anchor };
  }
  if (!toggle) return next;
  const indices = new Set(state.indices);
  if (indices.has(index)) indices.delete(index);
  else indices.add(index);
  return { indices, anchor: index };
}

export function voiceSongUp(
  state: VoiceSongSelection,
  index: number,
  event: { button: number; toggle: boolean; range: boolean },
): VoiceSongSelection {
  if (event.button !== 0) return state;
  if (!event.toggle && !event.range && state.collapse === index) return singleVoiceSong(index);
  return state.collapse === undefined ? state : { indices: state.indices, anchor: state.anchor };
}

export function voiceSongContext(
  state: VoiceSongSelection,
  index: number,
  modifiedPointer: boolean,
): VoiceSongSelection {
  return state.indices.has(index) || modifiedPointer ? state : singleVoiceSong(index);
}

export function allVoiceSongs(state: VoiceSongSelection, count: number): VoiceSongSelection {
  return {
    indices: new Set(Array.from({ length: count }, (_, index) => index)),
    anchor: state.anchor,
  };
}

/** Plain arrows leave a group from its top/bottom edge, including unmounted rows. */
export function voiceSongNeighbor(
  state: VoiceSongSelection,
  current: number,
  count: number,
  direction: -1 | 1,
): number {
  const base = state.indices.size
    ? direction < 0
      ? Math.min(...state.indices)
      : Math.max(...state.indices)
    : current;
  return Math.max(0, Math.min(count - 1, base + direction));
}

/** The menu owns this snapshot even if another row later receives focus. A
 * modified pointer menu outside the group falls back to its own single row. */
export function voiceSongSelectionSnapshot(
  state: VoiceSongSelection,
  index: number,
): readonly number[] | undefined {
  return state.indices.size > 1 && state.indices.has(index)
    ? Object.freeze([...state.indices])
    : undefined;
}
