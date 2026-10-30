import { computed, shallowReactive, type Ref } from 'vue';
import type { AccountView } from '../services/contracts.ts';
import type { ArtistTab, CatalogClient, CatalogQuery } from '../services/catalog.ts';
import { catalogPageKey } from '../services/catalogPages.ts';
import { useCore } from '../services/core.ts';
import { useCatalogPages } from './useCatalogPages.ts';
const kinds = {
  all: 'artist_songs',
  albums: 'artist_albums',
  videos: 'artist_videos',
  intro: 'artist_introduction',
  similar: 'similar_artists',
  concerts: 'artist_concerts',
} as const;
type ListTab = keyof typeof kinds;

/** Independent reads and continuation prefixes for the six artist panels. */
export function useArtistPages(
  tab: Ref<ArtistTab>,
  core: Pick<CatalogClient, 'catalogPage'> = useCore(),
  expired = () => {},
) {
  const slots = shallowReactive(new Map<ListTab, ReturnType<typeof useCatalogPages>>());
  const visited = shallowReactive(new Map<ArtistTab, string>());
  const firstQueries = new Map<ArtistTab, CatalogQuery>();
  const failedReads = new Map<ArtistTab, 'first' | 'more'>();
  let generation = 0;
  let account: AccountView | undefined;
  let disposed = false;
  function create() {
    for (const key of Object.keys(kinds) as ListTab[]) {
      const slot = useCatalogPages(core, expired);
      if (account) slot.connect(account);
      slots.set(key, slot);
    }
  }
  create();
  const selected = computed(() => (tab.value === 'songs' ? undefined : slots.get(tab.value)));
  const opened = computed(() => visited.has(tab.value));
  function connect(next: AccountView) {
    if (disposed) return;
    if (next.epoch !== account?.epoch || next.status !== account?.status) {
      generation++;
      visited.clear();
      firstQueries.clear();
      failedReads.clear();
    }
    account = next;
    for (const slot of slots.values()) slot.connect(next);
  }
  async function open(query: CatalogQuery, refresh = false) {
    if (disposed || tab.value === 'songs') return;
    const key = tab.value;
    if (query.kind !== kinds[key]) throw Error('歌手标签与查询不匹配');
    const identity = catalogPageKey(query);
    // Returning to a tab preserves its prefix and failure state. Only the
    // explicit refresh/retry action revalidates, without replaying any writes.
    if (!refresh && visited.get(key) === identity) return;
    visited.set(key, identity);
    const target = { ...query };
    firstQueries.set(key, target);
    const slot = slots.get(key),
      owner = generation;
    await slot?.open(target, refresh);
    if (owner === generation && slots.get(key) === slot) {
      if (slot?.error.value) failedReads.set(key, 'first');
      else failedReads.delete(key);
    }
  }
  async function more() {
    const key = tab.value,
      slot = selected.value,
      owner = generation;
    if (!slot || slot.loading.value || slot.loadingMore.value) return;
    await slot.more();
    if (owner === generation && slots.get(key as ListTab) === slot) {
      if (slot.error.value) failedReads.set(key, 'more');
      else failedReads.delete(key);
    }
  }
  async function retry() {
    const key = tab.value;
    if (failedReads.get(key) === 'more') await more();
    else {
      const query = firstQueries.get(key);
      if (query) await open(query, true);
    }
  }
  function clear() {
    if (disposed) return;
    generation++;
    visited.clear();
    firstQueries.clear();
    failedReads.clear();
    for (const slot of slots.values()) slot.dispose();
    slots.clear();
    create();
  }
  function dispose() {
    disposed = true;
    generation++;
    visited.clear();
    firstQueries.clear();
    failedReads.clear();
    for (const slot of slots.values()) slot.dispose();
    slots.clear();
  }
  return {
    pages: computed(() => selected.value?.pages.value ?? []),
    loading: computed(() => opened.value && !!selected.value?.loading.value),
    loadingMore: computed(() => opened.value && !!selected.value?.loadingMore.value),
    error: computed(() => (opened.value ? (selected.value?.error.value ?? '') : '')),
    connect,
    open,
    more,
    retry,
    clear,
    dispose,
  };
}
