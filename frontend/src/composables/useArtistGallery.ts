import { computed, ref, shallowRef, watch } from 'vue';
import type { ArtistGalleryFilter, ArtistGalleryPage, CatalogClient } from '../services/catalog';
import { failure, type AccountView } from '../services/contracts.ts';

const first = (): ArtistGalleryFilter => ({ area: -1, type: -1, initial: -1 });
const key = (filter: ArtistGalleryFilter) => [filter.area, filter.type, filter.initial].join(':');
const BATCH_SIZE = 200; // Source positions (ten native pages), not the number of visible cards.
type Batch = ArtistGalleryPage & { startOffset: number };
type PageRequest = { offset: number; startOffset: number };
const firstPage = (): Batch => ({ artists: [], nextOffset: 0, startOffset: 0 });

export function useArtistGallery(
  core: Pick<CatalogClient, 'artistGallery'>,
  account: () => AccountView,
  expired: () => void,
) {
  const filter = shallowRef(first());
  const page = shallowRef(firstPage());
  const pending = ref(false);
  const error = ref('');
  const batchNumber = computed(() => Math.floor(page.value.startOffset / BATCH_SIZE) + 1);
  const nextBatch = computed(
    () =>
      page.value.nextOffset !== null &&
      page.value.nextOffset >= page.value.startOffset + BATCH_SIZE,
  );
  // Keep short, account-local histories for the most recent filter combinations.
  const saved = new Map<string, Batch>();
  let generation = 0;
  let opener: string | undefined;
  let failed: PageRequest | undefined;
  watch(
    () => [account().epoch, account().status],
    () => {
      generation++;
      filter.value = first();
      page.value = firstPage();
      pending.value = false;
      error.value = '';
      saved.clear();
      opener = undefined;
      failed = undefined;
    },
    { flush: 'sync' },
  );
  async function read(target: PageRequest) {
    if (pending.value || account().status !== 'authenticated') return;
    const epoch = account().epoch;
    const request = ++generation;
    const query = { ...filter.value, offset: target.offset };
    const live = () =>
      request === generation && account().epoch === epoch && account().status === 'authenticated';
    pending.value = true;
    error.value = '';
    failed = undefined;
    try {
      const result = await core.artistGallery(epoch, query);
      if (!live() || result.epoch !== epoch) return;
      const next = result.value;
      const artists = target.startOffset === page.value.startOffset ? [...page.value.artists] : [];
      const known = new Set(artists.map((artist) => artist.id));
      for (const artist of next.artists) {
        if (known.has(artist.id)) continue;
        known.add(artist.id);
        artists.push(artist);
      }
      page.value = { ...next, artists, startOffset: target.startOffset };
      const identity = key(query);
      saved.delete(identity);
      saved.set(identity, page.value);
      if (saved.size > 8) saved.delete(saved.keys().next().value!);
    } catch (value) {
      if (!live()) return;
      const issue = failure(value);
      failed = target;
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (live()) pending.value = false;
    }
  }
  function load() {
    const { nextOffset, startOffset } = page.value;
    if (nextOffset !== null && !nextBatch.value && !failed)
      return read({ offset: nextOffset, startOffset });
  }
  function browse(direction: 'next' | 'previous') {
    const { nextOffset, startOffset } = page.value;
    const offset =
      direction === 'next'
        ? nextBatch.value
          ? nextOffset
          : null
        : startOffset >= BATCH_SIZE
          ? startOffset - BATCH_SIZE
          : null;
    if (offset !== null && !failed) return read({ offset, startOffset: offset });
  }
  function retry() {
    if (failed) return read(failed);
  }
  function select(patch: Partial<ArtistGalleryFilter>) {
    const next = { ...filter.value, ...patch };
    if (key(next) === key(filter.value)) return;
    generation++;
    const identity = key(next);
    const cached = saved.get(identity);
    if (cached) {
      saved.delete(identity);
      saved.set(identity, cached);
    }
    filter.value = next;
    page.value = cached ?? firstPage();
    pending.value = false;
    error.value = '';
    opener = undefined;
    failed = undefined;
    if (!cached) void load();
  }
  return {
    filter,
    page,
    pending,
    error,
    batchNumber,
    nextBatch,
    load,
    browse,
    retry,
    select,
    rememberOpener: (id: string) => (opener = id),
    takeOpener() {
      const id = opener;
      opener = undefined;
      return id;
    },
  };
}
