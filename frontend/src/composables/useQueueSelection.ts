import { computed, shallowRef, watch } from 'vue';
import {
  queueCollectionGesture,
  selectQueueRows,
  type QueueSelection,
} from '../services/queueSelection';
import type { QueueView } from '../services/player';

export function useQueueSelection(queue: () => QueueView | null | undefined) {
  const state = shallowRef<QueueSelection>({ selected: new Set() });
  const ids = computed(() => queue()?.tracks.map((item) => item.entryId) ?? []);
  watch([() => queue()?.epoch, ids], ([epoch, rows], [previousEpoch]) => {
    const previous = state.value;
    const available = new Set(rows);
    state.value =
      epoch !== previousEpoch
        ? { selected: new Set() }
        : {
            selected: new Set([...previous.selected].filter((id) => available.has(id))),
            anchor: previous.anchor && available.has(previous.anchor) ? previous.anchor : undefined,
          };
  });
  function select(id: string, toggle = false, range = false) {
    state.value = selectQueueRows(ids.value, state.value, id, toggle, range);
  }
  function all() {
    state.value = { selected: new Set(ids.value), anchor: ids.value[0] };
  }
  function clear() {
    state.value = { selected: new Set() };
  }
  function collect(id: string, pointer: boolean, toggle: boolean, range: boolean) {
    const action = queueCollectionGesture(ids.value, state.value, id, { pointer, toggle, range });
    if (!action) return;
    state.value = action.selection;
    return action.entryIds;
  }
  return { ids, selected: computed(() => state.value.selected), select, all, clear, collect };
}
