import { computed, ref, shallowRef } from 'vue';
import { failure } from '../services/contracts.ts';
import {
  appendLatestAlbums,
  latestCursorKey,
  latestPlayable,
  type LatestAlbumArea,
  type LatestAlbumGroup,
  type LatestReleaseClient,
  type LatestReleaseCursor,
  type LatestReleasePosition,
  type LatestReleaseQuery,
  type LatestReleaseTab,
  type LatestSongArea,
  type LatestSongPage,
} from '../services/latestReleases.ts';

interface Saved {
  songArea: LatestSongArea;
  songs: Map<LatestSongArea, LatestSongPage>;
  albumArea: LatestAlbumArea;
  recommended: boolean;
  albumGroups: LatestAlbumGroup[];
  next: LatestReleaseCursor | null;
  albumLoaded: boolean;
  emptyContinuations: number;
  seen: Set<string>;
  position: LatestReleasePosition;
}

export function createLatestReleaseMemory() {
  const visits = new Map<string, Saved>();
  let account: string | null = null;
  return {
    connect(epoch: string | null) {
      if (epoch !== account) visits.clear();
      account = epoch;
    },
    read(epoch: string, visit: number, tab: LatestReleaseTab) {
      if (epoch === account) return visits.get(`${visit}:${tab}`);
    },
    save(epoch: string, visit: number, tab: LatestReleaseTab, state: Saved) {
      if (epoch !== account) return;
      const key = `${visit}:${tab}`;
      visits.delete(key);
      visits.set(key, state);
      if (visits.size > 10) visits.delete(visits.keys().next().value!);
    },
  };
}

export function useLatestReleases(
  core: LatestReleaseClient,
  memory: ReturnType<typeof createLatestReleaseMemory>,
  expired: () => void,
) {
  const songArea = ref<LatestSongArea>('0');
  const songs = shallowRef(new Map<LatestSongArea, LatestSongPage>());
  const albumArea = ref<LatestAlbumArea>('ALL');
  const recommended = ref(true);
  const albumGroups = shallowRef<LatestAlbumGroup[]>([]);
  const next = shallowRef<LatestReleaseCursor | null>(null);
  const albumLoaded = ref(false);
  const emptyContinuations = ref(0);
  const pending = ref(false);
  const error = ref('');
  const tab = ref<LatestReleaseTab>('songs');
  let seen = new Set<string>();
  let owner: { epoch: string; visit: number } | undefined;
  let generation = 0;
  const songPage = computed(() => songs.value.get(songArea.value));
  const playable = computed(() => (songPage.value ? latestPlayable(songPage.value) : []));
  const ready = computed(() => (tab.value === 'songs' ? !!songPage.value : albumLoaded.value));
  const autoMore = computed(() => !!next.value && emptyContinuations.value < 3);

  function open(epoch: string, visit: number, requested: LatestReleaseTab) {
    generation++;
    owner = { epoch, visit };
    tab.value = requested;
    const saved = memory.read(epoch, visit, requested);
    // The original keeps the selected regions independently of the active
    // route. A tab’s populated visit also survives opening a card and Back.
    const other = memory.read(epoch, visit, requested === 'songs' ? 'albums' : 'songs');
    songArea.value = (saved ?? other)?.songArea ?? '0';
    songs.value = new Map((saved ?? other)?.songs);
    albumArea.value = (saved ?? other)?.albumArea ?? 'ALL';
    recommended.value = (saved ?? other)?.recommended ?? true;
    albumGroups.value = (saved ?? other)?.albumGroups ?? [];
    next.value = (saved ?? other)?.next ?? null;
    albumLoaded.value = (saved ?? other)?.albumLoaded ?? false;
    emptyContinuations.value = (saved ?? other)?.emptyContinuations ?? 0;
    seen = new Set((saved ?? other)?.seen);
    pending.value = false;
    error.value = '';
    // M3146 fetchInitData reloads the selected song region on every mount.
    // Keep the cached table usable while the replacement is in flight.
    if (requested === 'songs' || !ready.value) void load();
    return saved?.position;
  }

  function leave(position: LatestReleasePosition) {
    const current = owner;
    if (!current) return;
    memory.save(current.epoch, current.visit, tab.value, {
      songArea: songArea.value,
      songs: songs.value,
      albumArea: albumArea.value,
      recommended: recommended.value,
      albumGroups: albumGroups.value,
      next: next.value,
      albumLoaded: albumLoaded.value,
      emptyContinuations: emptyContinuations.value,
      seen,
      position,
    });
    owner = undefined;
    generation++;
    pending.value = false;
  }

  async function load(more = false) {
    const current = owner;
    if (!current || pending.value || (more && (tab.value !== 'albums' || !next.value))) return;
    const ticket = ++generation;
    const query: LatestReleaseQuery =
      tab.value === 'songs'
        ? { kind: 'songs', area: songArea.value }
        : {
            kind: 'albums',
            area: albumArea.value,
            recommended: recommended.value,
            cursor: more ? next.value : null,
          };
    const valid = () => ticket === generation && owner === current;
    pending.value = true;
    error.value = '';
    try {
      const reply = await core.latestReleases(current.epoch, query);
      if (!valid() || reply.epoch !== current.epoch) return;
      const value = reply.value;
      if (value.kind === 'songs' && query.kind === 'songs') {
        songs.value = new Map(songs.value).set(query.area, value);
      } else if (value.kind === 'albums' && query.kind === 'albums') {
        const accepted = more ? new Set(seen) : new Set<string>();
        accepted.add(latestCursorKey(query.cursor));
        const previous = more ? albumGroups.value : [];
        const previousCount = previous.reduce((count, group) => count + group.albums.length, 0);
        const groups = appendLatestAlbums(previous, value);
        const count = groups.reduce((total, group) => total + group.albums.length, 0);
        emptyContinuations.value = count > previousCount ? 0 : emptyContinuations.value + 1;
        albumGroups.value = groups;
        next.value = value.next && !accepted.has(latestCursorKey(value.next)) ? value.next : null;
        seen = accepted;
        albumLoaded.value = true;
      } else {
        error.value = '最新音乐返回了无法读取的内容，请重试。';
      }
    } catch (reason) {
      if (!valid()) return;
      const issue = failure(reason);
      error.value = issue.message;
      if (issue.code === 'session_expired' || issue.code === 'profile_changed') expired();
    } finally {
      if (valid()) pending.value = false;
    }
  }

  function chooseSong(area: LatestSongArea) {
    if (area === songArea.value) {
      if (error.value) void load();
      return false;
    }
    generation++;
    pending.value = false;
    error.value = '';
    songArea.value = area;
    if (!songPage.value?.tracks.length) void load();
    return true;
  }

  function chooseAlbum(area: LatestAlbumArea, preference = recommended.value) {
    if (pending.value || (area === albumArea.value && preference === recommended.value))
      return false;
    albumArea.value = area;
    recommended.value = preference;
    albumGroups.value = [];
    albumLoaded.value = false;
    emptyContinuations.value = 0;
    next.value = null;
    seen = new Set();
    void load();
    return true;
  }

  function retry() {
    return load(tab.value === 'albums' && albumLoaded.value && next.value !== null);
  }

  return {
    tab,
    songArea,
    songPage,
    playable,
    albumArea,
    recommended,
    albumGroups,
    next,
    pending,
    ready,
    autoMore,
    error,
    open,
    leave,
    load,
    chooseSong,
    chooseAlbum,
    retry,
  };
}
