import { computed, ref, shallowRef } from 'vue';
import type {
  AccountData,
  AccountView,
  CoreClient,
  PlaylistView,
  TrackPage,
  TrackView,
} from '../services/contracts.ts';
import { failure } from '../services/contracts.ts';
import { useCore } from '../services/core.ts';
import type { CommentResource } from '../services/comments';
export type Page =
  | { kind: 'settings' }
  | { kind: 'history'; scope: import('../services/history').HistoryKind }
  | { kind: 'collections'; scope: import('../services/collections').CollectionScope }
  | { kind: 'catalog'; resource: import('../services/catalog').CatalogResource }
  | { kind: 'scene_fm' }
  | { kind: 'member' }
  | { kind: 'fm' }
  | { kind: 'rankings'; query: import('../services/rankings').RankingQuery }
  | { kind: 'together' }
  | { kind: 'podcasts' }
  | { kind: 'local' }
  | { kind: 'daily' }
  | { kind: 'library' }
  | { kind: 'downloads' }
  | { kind: 'cloud' }
  | { kind: 'mvs' }
  | { kind: 'mv'; id: string; taskId?: string }
  | { kind: 'comments'; resource: CommentResource; title: string }
  | { kind: 'playlist'; id: string }
  | { kind: 'search'; keyword: string };

export function useMusic(core: CoreClient = useCore()) {
  const account = ref<AccountView>({
    epoch: '0',
    status: 'unknown',
    nickname: null,
    avatarUrl: null,
  });
  const playlists = shallowRef<PlaylistView[]>([]);
  const liked = shallowRef(new Set<string>());
  const busyLikes = shallowRef(new Set<string>());
  const page = shallowRef<Page>({ kind: 'daily' });
  const result = shallowRef<TrackPage>();
  const loading = ref(false);
  const initializing = ref(true);
  const error = ref('');
  const notice = ref('');
  const libraryError = ref('');
  const history: Page[] = [];
  const historyLength = ref(0);
  let request = 0;
  let accountRequest = 0;
  let resourceRequest = 0;
  let likeRevision = 0;
  const signedIn = computed(() => account.value.status === 'authenticated');
  function accept<T>(data: AccountData<T>): T | undefined {
    return data.epoch === account.value.epoch && signedIn.value ? data.value : undefined;
  }
  function handle(e: unknown) {
    const issue = failure(e);
    error.value = issue.message;
    if (issue.code === 'session_expired' || issue.code === 'profile_changed') {
      request++;
      account.value = { ...account.value, status: 'expired', nickname: null, avatarUrl: null };
      playlists.value = [];
      liked.value = new Set();
      result.value = undefined;
    }
  }
  async function resources() {
    const current = ++resourceRequest;
    const likesAtStart = likeRevision;
    libraryError.value = '';
    const replies = await Promise.allSettled([core.playlists(), core.likedIds()]);
    if (current !== resourceRequest) return;
    const lists = replies[0];
    if (lists.status === 'fulfilled') {
      const v = accept(lists.value);
      if (v) playlists.value = v;
    } else {
      libraryError.value = failure(lists.reason).message;
      handle(lists.reason);
    }
    const likes = replies[1];
    if (likes.status === 'fulfilled') {
      const v = accept(likes.value);
      if (v && likesAtStart === likeRevision) liked.value = new Set(v);
    } else {
      libraryError.value = failure(likes.reason).message;
      handle(likes.reason);
    }
  }
  async function load(more = false) {
    const current = ++request;
    error.value = '';
    loading.value = true;
    const target = page.value;
    const offset = more ? (result.value?.tracks.length ?? 0) : 0;
    if (!more) result.value = undefined;
    try {
      if (
        !signedIn.value ||
        target.kind === 'library' ||
        target.kind === 'history' ||
        target.kind === 'settings' ||
        target.kind === 'local' ||
        target.kind === 'downloads' ||
        target.kind === 'cloud' ||
        target.kind === 'mvs' ||
        target.kind === 'podcasts' ||
        target.kind === 'together' ||
        target.kind === 'rankings' ||
        target.kind === 'fm' ||
        target.kind === 'scene_fm' ||
        target.kind === 'member' ||
        target.kind === 'catalog' ||
        target.kind === 'collections' ||
        target.kind === 'mv' ||
        target.kind === 'comments'
      )
        return;
      const data =
        target.kind === 'daily'
          ? await core.dailySongs()
          : target.kind === 'playlist'
            ? await core.playlistPage(target.id, {
                offset,
                limit: 100,
                revision: more ? result.value?.revision : null,
              })
            : await core.searchSongs(target.keyword, { offset, limit: 100 });
      if (current !== request) return;
      const value = accept(data);
      if (!value) return;
      result.value =
        more && result.value
          ? { ...value, tracks: [...result.value.tracks, ...value.tracks] }
          : value;
    } catch (e) {
      if (current === request) handle(e);
    } finally {
      if (current === request) loading.value = false;
    }
  }
  async function bootstrap(restoring = true) {
    const current = ++accountRequest;
    request++;
    resourceRequest++;
    initializing.value = true;
    error.value = '';
    try {
      const next = restoring ? await core.restoreAccount() : await core.accountSnapshot();
      if (current !== accountRequest) return;
      if (next.epoch !== account.value.epoch) {
        playlists.value = [];
        liked.value = new Set();
        result.value = undefined;
        busyLikes.value = new Set();
      }
      account.value = next;
      if (signedIn.value) {
        await resources();
        if (current !== accountRequest) return;
        await load();
      }
    } catch (e) {
      if (current === accountRequest) handle(e);
    } finally {
      if (current === accountRequest) initializing.value = false;
    }
  }
  function navigate(next: Page) {
    if (initializing.value) return;
    history.push(page.value);
    historyLength.value = history.length;
    page.value = next;
    void load();
  }
  function back() {
    const previous = history.pop();
    historyLength.value = history.length;
    if (previous) {
      page.value = previous;
      void load();
    }
  }
  function replace(next: Page) {
    if (initializing.value) return;
    page.value = next;
    void load();
  }
  async function like(track: TrackView) {
    if (busyLikes.value.has(track.id)) return;
    busyLikes.value = new Set(busyLikes.value).add(track.id);
    notice.value = '';
    likeRevision++;
    try {
      const response = await core.setLiked(
        account.value.epoch,
        track.id,
        !liked.value.has(track.id),
      );
      const value = accept(response);
      if (!value) return;
      if (value.observed !== null) {
        const next = new Set(liked.value);
        value.observed ? next.add(track.id) : next.delete(track.id);
        liked.value = next;
      }
      if (value.phase !== 'confirmed' && value.phase !== 'unchanged')
        notice.value =
          value.phase === 'rejected'
            ? '服务未确认此次操作，请刷新后核对'
            : '操作结果尚未确认，请刷新核对后再操作';
    } catch (e) {
      handle(e);
    } finally {
      likeRevision++;
      const next = new Set(busyLikes.value);
      next.delete(track.id);
      busyLikes.value = next;
    }
  }
  return {
    replace,
    account,
    playlists,
    liked,
    busyLikes,
    page,
    result,
    loading,
    initializing,
    error,
    notice,
    libraryError,
    historyLength,
    signedIn,
    resources,
    load,
    bootstrap,
    navigate,
    back,
    like,
  };
}
