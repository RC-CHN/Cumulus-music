import { computed, ref, shallowRef } from 'vue';
import type { CoreClient, PlaylistTagPage, PlaylistTagQuery } from '../services/contracts';

const pageSize = 24;

export function usePlaylistTagBrowse(client: Pick<CoreClient, 'playlistTagPage'>) {
  const items = shallowRef<PlaylistTagPage['items'] | null>(null);
  const busy = ref(false);
  const failed = ref(false);
  const offset = ref(0);
  const lastOpened = ref('');
  const remaining = shallowRef<string[]>([]);
  const canNext = computed(() => offset.value < remaining.value.length);
  let active = false;
  let epoch = '';
  let tag = '';
  let generation = 0;
  let requested = 0;
  let first: PlaylistTagPage['items'] = [];

  async function read(target: number) {
    if (!active || busy.value) return;
    if (target === 0 && items.value !== null) {
      items.value = first;
      offset.value = 0;
      failed.value = false;
      return;
    }
    const query: PlaylistTagQuery = target
      ? { kind: 'items', ids: remaining.value.slice(target - pageSize, target) }
      : { kind: 'category', tag };
    if (query.kind === 'items' && !query.ids.length) return;
    const ticket = ++generation;
    requested = target;
    busy.value = true;
    failed.value = false;
    try {
      const reply = await client.playlistTagPage(epoch, query);
      if (ticket !== generation) return;
      if (reply.epoch !== epoch) {
        failed.value = true;
        return;
      }
      if (target === 0) {
        first = reply.value.items;
        remaining.value = reply.value.remainingIds;
      }
      items.value = reply.value.items;
      offset.value = target;
    } catch {
      if (ticket === generation) failed.value = true;
    } finally {
      if (ticket === generation) busy.value = false;
    }
  }

  function enter(owner: string, category: string | null) {
    generation++;
    busy.value = false;
    failed.value = false;
    active = !!owner && !!category;
    if (epoch !== owner || (category !== null && tag !== category)) {
      epoch = owner;
      tag = category ?? '';
      items.value = null;
      first = [];
      remaining.value = [];
      offset.value = requested = 0;
      lastOpened.value = '';
    }
    if (active && items.value === null) return read(0);
  }

  return {
    items,
    busy,
    failed,
    offset,
    canNext,
    lastOpened,
    enter,
    retry: () => read(requested),
    previous: () => offset.value > 0 && read(offset.value - pageSize),
    next: () => canNext.value && read(offset.value + pageSize),
  };
}
