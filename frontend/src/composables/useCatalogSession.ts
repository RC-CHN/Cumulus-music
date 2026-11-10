import { commentStartPosition } from '../services/comments.ts';
import { computed, ref } from 'vue';
import type { AlbumTrackOrder } from '../services/albumDurationSort.ts';
import type { AccountView, CoreClient } from '../services/contracts.ts';
import type {
  ArtistTab,
  CatalogResource,
  CatalogSelectionPage,
  CatalogMembershipListener,
} from '../services/catalog.ts';
import {
  artistQuery,
  type AlbumTab,
  type CatalogLocation,
  type CatalogPanelKey,
  type CatalogPosition,
} from '../services/catalogNavigation.ts';
import { artistHasMvs, artistVideoSlots, nextCatalogQuery } from '../services/catalogPages.ts';
import { useCatalog } from './useCatalog.ts';
import { useCatalogMembership } from './useCatalogMembership.ts';
import { useArtistPages } from './useArtistPages.ts';
import { useComments } from './useComments.ts';
import { useCommentContext } from './useCommentContext.ts';

/** A single bounded browsing visit; this owns models, never DOM or open dialogs. */
export function useCatalogSession(
  account: AccountView,
  resource: CatalogResource,
  core: CoreClient,
  expired: () => void,
  membershipChanged: CatalogMembershipListener = () => {},
  albumOrder = ref<AlbumTrackOrder>('original'),
) {
  resource = { ...resource };
  const owner = account.epoch;
  const tab = ref<ArtistTab>('songs'),
    albumTab = ref<AlbumTab>('songs');
  const positions = new Map<CatalogPanelKey, CatalogPosition>();
  const detail = useCatalog(core, expired),
    dynamic = useCatalog(core, expired),
    membership = useCatalogMembership(core, expired),
    list = useArtistPages(tab, core, expired);
  const comments = useComments(core, expired);
  const commentContext = useCommentContext(comments, core);
  const observed = ref<boolean | null>(null);
  let disposed = false;
  detail.connect(account);
  dynamic.connect(account);
  membership.connect(account, resource);
  list.connect(account);
  comments.connect(account);
  const artist = computed(() =>
    detail.view.value?.page.kind === 'artist' ? detail.view.value.page.artist : undefined,
  );
  const album = computed(() =>
    detail.view.value?.page.kind === 'album' ? detail.view.value.page.album : undefined,
  );
  const hotTracks = computed(() =>
    detail.view.value?.page.kind === 'artist' ? detail.view.value.page.tracks : [],
  );
  const onlineCount = computed(() =>
    dynamic.view.value?.page.kind === 'artist_dynamic'
      ? dynamic.view.value.page.onlineCount
      : undefined,
  );
  const subscribed = computed(() =>
    observed.value !== null
      ? observed.value
      : resource.kind === 'album'
        ? (album.value?.subscribed ?? null)
        : dynamic.view.value?.page.kind === 'artist_dynamic'
          ? dynamic.view.value.page.followed
          : null,
  );
  const stateReading = computed(() =>
    resource.kind === 'album'
      ? detail.loading.value || !!detail.error.value
      : dynamic.loading.value || !!dynamic.error.value,
  );
  const tracks = computed(() => {
    const page = detail.view.value?.page;
    if (tab.value === 'songs')
      return page?.kind === 'artist' || page?.kind === 'album' ? page.tracks : [];
    return tab.value === 'all'
      ? list.pages.value.flatMap((view) =>
          view.page.kind === 'artist_songs' ? view.page.tracks : [],
        )
      : [];
  });
  const selections = computed<CatalogSelectionPage[]>(() =>
    (tab.value === 'songs' ? (detail.view.value ? [detail.view.value] : []) : list.pages.value).map(
      (view) => ({ query: view.query, revision: view.revision }),
    ),
  );
  const next = computed(() => {
    const last = list.pages.value.at(-1);
    return last ? nextCatalogQuery(last) : null;
  });
  const intro = computed(() =>
    list.pages.value[0]?.page.kind === 'artist_introduction' ? list.pages.value[0].page : undefined,
  );
  const auxiliary = computed(() =>
    detail.view.value?.page.kind === 'album' ? detail.view.value.page.read : undefined,
  );
  const artistMvAvailable = computed(() => artistHasMvs(artist.value?.mvCount));
  const artistVideos = computed(() => artistVideoSlots(list.pages.value));
  const cards = computed(() => ({
    albums: list.pages.value.flatMap((view) =>
      view.page.kind === 'artist_albums' ? view.page.albums : [],
    ),
    artists: list.pages.value.flatMap((view) =>
      view.page.kind === 'similar_artists' ? view.page.artists : [],
    ),
    concerts: list.pages.value.flatMap((view) =>
      view.page.kind === 'artist_concerts' ? view.page.concerts : [],
    ),
  }));
  async function readArtistTab(refresh = false) {
    if (disposed || (tab.value === 'videos' && !artistMvAvailable.value)) return;
    const query = artistQuery(tab.value, resource.id);
    if (query) await list.open(query, refresh);
  }
  async function select(location: CatalogLocation) {
    if (disposed) return;
    if (resource.kind === 'artist') {
      tab.value = location.artistTab ?? 'songs';
      await readArtistTab();
    } else {
      albumTab.value = location.albumTab ?? 'songs';
      if (albumTab.value === 'comments' && comments.resource.value?.id !== resource.id)
        await comments.open(
          { kind: 'album', id: resource.id },
          commentStartPosition(location.initialCommentId),
        );
    }
  }
  async function refreshState() {
    await Promise.all([
      membership.command({ kind: 'inspect' }),
      resource.kind === 'artist'
        ? dynamic.open({ kind: 'artist_dynamic', id: resource.id }, true)
        : Promise.resolve(),
    ]);
  }
  async function activate() {
    if (disposed) return;
    observed.value = null;
    await Promise.all([
      detail.open(resource),
      refreshState(),
      resource.kind === 'album' && albumTab.value === 'comments' && comments.page.value
        ? comments.load()
        : Promise.resolve(),
    ]);
    // A restored or evicted artist visit can be targeting MV before its own
    // artist header returns. Never request the tab without a positive mvSize.
    if (resource.kind === 'artist' && tab.value === 'videos') await readArtistTab();
  }
  async function refresh() {
    if (disposed) return;
    observed.value = null;
    const mv = resource.kind === 'artist' && tab.value === 'videos';
    await Promise.all([
      detail.open(resource, true),
      refreshState(),
      resource.kind === 'artist' && !mv ? readArtistTab(true) : Promise.resolve(),
    ]);
    if (mv && tab.value === 'videos') await readArtistTab(true);
  }
  async function collect(reconcile = false) {
    if (disposed || (!reconcile && (subscribed.value === null || stateReading.value))) return;
    membershipChanged(owner, resource);
    let latest: boolean | undefined;
    try {
      const result = await membership.command(
        reconcile ? { kind: 'reconcile' } : { kind: 'set', desired: !subscribed.value },
      );
      if (!disposed && result !== null) observed.value = result;
      latest = result ?? undefined;
      return !disposed ? result : null;
    } finally {
      // This epoch-bound listener outlives an unmounted page or evicted model.
      membershipChanged(owner, resource, latest);
    }
  }
  function dispose() {
    if (disposed) return;
    disposed = true;
    positions.clear();
    observed.value = null;
    detail.dispose();
    dynamic.dispose();
    list.dispose();
    membership.dispose();
    commentContext.dispose();
    comments.connect({ ...account, status: 'signed_out' });
  }
  return {
    valid: () => !disposed,
    resource,
    tab,
    albumTab,
    albumOrder,
    positions,
    detail,
    dynamic,
    membership,
    list,
    comments,
    commentContext,
    artist,
    album,
    hotTracks,
    onlineCount,
    subscribed,
    stateReading,
    tracks,
    selections,
    next,
    intro,
    auxiliary,
    cards,
    artistMvAvailable,
    artistVideos,
    select,
    activate,
    refreshState,
    refresh,
    collect,
    dispose,
  };
}
export type CatalogSession = ReturnType<typeof useCatalogSession>;
