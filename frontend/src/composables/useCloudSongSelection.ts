import { shallowRef, watch } from 'vue';
import { cloudSongSelection } from '../services/cloudSongs.ts';
/** Keep controlled row events synchronous even before Vue patches new props. */
export function useCloudSongSelection(
  ids: () => string[],
  external: () => Set<string>,
  changed: (value: Set<string>) => void,
) {
  const selected = shallowRef(external());
  let anchor: string | undefined;
  watch(
    external,
    (value) => {
      selected.value = value;
    },
    { flush: 'sync' },
  );
  function update(value: Set<string>) {
    selected.value = value;
    changed(value);
  }
  function choose(index: number, ctrl = false, shift = false) {
    const value = cloudSongSelection(ids(), selected.value, index, anchor, { ctrl, shift });
    anchor = value.anchor;
    update(value.selected);
  }
  function all() {
    update(selected.value.size === ids().length ? new Set() : new Set(ids()));
  }
  return { selected, choose, update, all };
}
