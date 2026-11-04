import { computed, ref, shallowRef } from 'vue';
import type { CoreClient, PlaylistTagPage, PlaylistTagQuery } from '../services/contracts';

const PAGE_SIZE = 24;
interface TagSnapshot {
  items: PlaylistTagPage['items'];
  remaining: string[];
  offset: number;
  essential: PlaylistTagPage['essential'];
  manual: boolean;
  failed: boolean;
  scroll: number;
  opener: string;
}

export function usePlaylistTagBrowse(client: Pick<CoreClient, 'playlistTagPage'>) {
  const data = shallowRef<TagSnapshot>();
  const busy = ref(false),
    failed = ref(false);
  const category = ref('');
  let epoch = '',
    generation = 0;
  const saved = new Map<string, TagSnapshot>();
  const items = computed(() => data.value?.items ?? null);
  const essential = computed(() => data.value?.essential);
  const manual = computed(() => data.value?.manual ?? false);
  const canNext = computed(() => !!data.value && data.value.offset < data.value.remaining.length);

  async function load() {
    const tag = category.value,
      owner = epoch;
    if (!tag || !owner || busy.value || (data.value && !canNext.value)) return;
    const previous = data.value;
    const query: PlaylistTagQuery = previous
      ? {
          kind: 'items',
          ids: previous.remaining.slice(previous.offset, previous.offset + PAGE_SIZE),
        }
      : { kind: 'category', tag };
    const ticket = ++generation;
    busy.value = true;
    failed.value = false;
    try {
      const reply = await client.playlistTagPage(owner, query);
      if (ticket !== generation) return;
      if (reply.epoch !== owner) {
        failed.value = true;
        if (previous) previous.failed = true;
        return;
      }
      if (previous && query.kind === 'items') {
        const known = new Set(previous.items.map((item) => item.id));
        const fresh = reply.value.items.filter((item) => !known.has(item.id));
        data.value = {
          ...previous,
          items: [...previous.items, ...fresh],
          offset: previous.offset + query.ids.length,
          manual: fresh.length === 0,
          failed: false,
        };
      } else {
        data.value = {
          items: reply.value.items,
          remaining: reply.value.remainingIds,
          essential: reply.value.essential,
          offset: 0,
          manual: false,
          failed: false,
          scroll: 0,
          opener: '',
        };
      }
      saved.delete(tag);
      saved.set(tag, data.value);
      if (saved.size > 8) saved.delete(saved.keys().next().value!);
    } catch {
      if (ticket === generation) {
        failed.value = true;
        if (previous) previous.failed = true;
      }
    } finally {
      if (ticket === generation) busy.value = false;
    }
  }

  function enter(owner: string, next: string | null) {
    generation++;
    busy.value = false;
    failed.value = false;
    if (epoch !== owner) {
      epoch = owner;
      saved.clear();
    }
    category.value = next ?? '';
    data.value = next ? saved.get(next) : undefined;
    failed.value = data.value?.failed ?? false;
    if (next && !data.value) return load();
  }

  return {
    items,
    busy,
    failed,
    essential,
    canNext,
    manual,
    category,
    enter,
    load,
    saveScroll(tag: string, scroll: number) {
      if (tag === category.value && data.value) data.value.scroll = scroll;
    },
    remember(id: string) {
      if (data.value) data.value.opener = id;
    },
    restore() {
      const state = data.value;
      const result = { scroll: state?.scroll ?? 0, opener: state?.opener ?? '' };
      if (state) state.opener = '';
      return result;
    },
  };
}
