import { ref, shallowRef, watch } from 'vue';
import type { ArtistGalleryFilter, ArtistGalleryPage, CatalogClient } from '../services/catalog';
import { failure, type AccountView } from '../services/contracts.ts';

const first = (): ArtistGalleryFilter => ({ area: -1, type: -1, initial: -1 });
const key = (filter: ArtistGalleryFilter) => [filter.area, filter.type, filter.initial].join(':');
const firstPage = (): ArtistGalleryPage => ({ artists: [], nextOffset: 0, capped: false });

export function useArtistGallery(
  core: Pick<CatalogClient, 'artistGallery'>,
  account: () => AccountView,
  expired: () => void,
) {
  const filter = shallowRef(first());
  const page = shallowRef(firstPage());
  const pending = ref(false);
  const error = ref('');
  // Keep short, account-local histories for the most recent filter combinations.
  const saved = new Map<string, ArtistGalleryPage>();
  let generation = 0;
  let opener: string | undefined;
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
    },
    { flush: 'sync' },
  );
  async function load() {
    const offset = page.value.nextOffset;
    if (offset === null || pending.value || account().status !== 'authenticated') return;
    const epoch = account().epoch;
    const request = ++generation;
    const query = { ...filter.value, offset };
    const live = () =>
      request === generation && account().epoch === epoch && account().status === 'authenticated';
    pending.value = true;
    error.value = '';
    try {
      const result = await core.artistGallery(epoch, query);
      if (!live() || result.epoch !== epoch) return;
      const next = result.value;
      const known = new Set(page.value.artists.map((artist) => artist.id));
      const artists = [...page.value.artists];
      for (const artist of next.artists) {
        if (known.has(artist.id)) continue;
        known.add(artist.id);
        artists.push(artist);
      }
      page.value = { ...next, artists };
      const identity = key(query);
      saved.delete(identity);
      saved.set(identity, page.value);
      if (saved.size > 8) saved.delete(saved.keys().next().value!);
    } catch (value) {
      if (!live()) return;
      const issue = failure(value);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (live()) pending.value = false;
    }
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
    if (!cached) void load();
  }
  return {
    filter,
    page,
    pending,
    error,
    load,
    select,
    rememberOpener: (id: string) => (opener = id),
    takeOpener() {
      const id = opener;
      opener = undefined;
      return id;
    },
  };
}
