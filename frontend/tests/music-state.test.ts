import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useMusic } from '../src/composables/useMusic.ts';
import type { AccountData, CoreClient, TrackPage, TrackView } from '../src/services/contracts.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const song: TrackView = {
  id: '1',
  name: 'test',
  artists: [],
  album: '',
  coverUrl: null,
  durationMs: 1000,
  fee: 0,
  missing: false,
  local: false,
};
const page: TrackPage = {
  revision: null,
  tracks: [song],
  total: 1,
  offset: 0,
  more: false,
  playlist: null,
};
const data = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
function backend(overrides: Partial<CoreClient> = {}): CoreClient {
  const unused = async (): Promise<never> => {
    throw new Error('player is outside this test');
  };
  return {
    playlistTags: unused,
    playlistEditor: unused,
    playlistOrderView: unused,
    mutatePlaylist: unused,
    reconcilePlaylist: unused,
    playlistOperations: unused,
    watchPlayer: unused,
    playSelection: unused,
    playerAction: unused,
    playerQueue: unused,
    playerLyrics: unused,
    lyricPreferences: unused,
    lyricOffset: unused,
    getAppInfo: async () => ({ name: 'Cumulus', coreVersion: '0.1.0', protocolVersion: 1 }),
    accountSnapshot: async () => ({
      epoch: '1',
      status: 'authenticated',
      nickname: 'test',
      avatarUrl: null,
    }),
    restoreAccount: async () => ({
      epoch: '1',
      status: 'authenticated',
      nickname: 'test',
      avatarUrl: null,
    }),
    playlists: async () => data([]),
    likedIds: async () => data([]),
    dailySongs: async () => data(page),
    playlistPage: async () => data(page),
    searchSongs: async () => data(page),
    setLiked: async (_epoch, id, liked) => data({ id, phase: 'confirmed', observed: liked }),
    beginQr: async () => ({ operationId: 'x', status: 'waiting_scan', modules: [] }),
    pollQr: async () => 'waiting_scan',
    cancelQr: async () => {},
    ...overrides,
  };
}
const turn = () => new Promise((resolve) => setImmediate(resolve));

test('navigation during restore cannot invalidate the initial identity response', async () => {
  const identity = deferred<Awaited<ReturnType<CoreClient['restoreAccount']>>>();
  const music = useMusic(backend({ restoreAccount: () => identity.promise }));
  const ready = music.bootstrap();
  music.navigate({ kind: 'search', keyword: 'early' });
  identity.resolve({ epoch: '1', status: 'authenticated', nickname: 'test', avatarUrl: null });
  await ready;
  assert.equal(music.signedIn.value, true);
  assert.equal(music.initializing.value, false);
  assert.equal(music.page.value.kind, 'daily');
});

test('an old account resource read cannot unlock or load over a newer account notification', async () => {
  const oldLikes = deferred<AccountData<string[]>>();
  const nextAccount = deferred<Awaited<ReturnType<CoreClient['accountSnapshot']>>>();
  let reads = 0;
  const music = useMusic(
    backend({
      accountSnapshot: () => nextAccount.promise,
      likedIds: () => (++reads === 1 ? oldLikes.promise : Promise.resolve(data([], '2'))),
    }),
  );
  const first = music.bootstrap();
  await turn();
  const second = music.bootstrap(false);
  oldLikes.resolve(data(['old-private-like']));
  await first;
  assert.equal(music.initializing.value, true);
  assert.equal(music.result.value, undefined);
  nextAccount.resolve({ epoch: '2', status: 'authenticated', nickname: 'new', avatarUrl: null });
  await second;
  assert.equal(music.account.value.epoch, '2');
  assert.equal(music.initializing.value, false);
  assert.equal(music.liked.value.has('old-private-like'), false);
});

test('a slow search cannot overwrite a later page', async () => {
  const old = deferred<AccountData<TrackPage>>();
  const music = useMusic(
    backend({
      searchSongs: (keyword) =>
        keyword === 'old'
          ? old.promise
          : Promise.resolve(data({ ...page, tracks: [{ ...song, name: 'new' }] })),
    }),
  );
  await music.bootstrap();
  music.navigate({ kind: 'search', keyword: 'old' });
  music.navigate({ kind: 'search', keyword: 'new' });
  await turn();
  old.resolve(data(page));
  await turn();
  assert.equal(music.result.value?.tracks[0].name, 'new');
});

test('a pending resource refresh cannot undo a confirmed like and writes carry their account epoch', async () => {
  const likes = deferred<AccountData<string[]>>();
  let refreshing = false;
  let epoch = '';
  const music = useMusic(
    backend({
      likedIds: () => (refreshing ? likes.promise : Promise.resolve(data([]))),
      setLiked: async (e, id, liked) => {
        epoch = e;
        return data({ id, phase: 'confirmed', observed: liked });
      },
    }),
  );
  await music.bootstrap();
  refreshing = true;
  const refresh = music.resources();
  await music.like(song);
  likes.resolve(data([]));
  await refresh;
  assert.equal(epoch, '1');
  assert.equal(music.liked.value.has('1'), true);
});

test('301 clears private views and old-epoch results never appear', async () => {
  const music = useMusic(
    backend({
      searchSongs: async (keyword) => {
        if (keyword === 'expired')
          throw { code: 'session_expired', message: '重新登录', retryable: false };
        return data(page, '0');
      },
    }),
  );
  await music.bootstrap();
  music.navigate({ kind: 'search', keyword: 'old-epoch' });
  await turn();
  assert.equal(music.result.value, undefined);
  music.navigate({ kind: 'search', keyword: 'expired' });
  await turn();
  assert.equal(music.signedIn.value, false);
  assert.equal(music.playlists.value.length, 0);
  assert.equal(music.liked.value.size, 0);
  assert.equal(music.result.value, undefined);
});

test('native like read-back cannot overwrite a newer UI like or a replacement account', async () => {
  const old = deferred<AccountData<string[]>>();
  let count = 0;
  const music = useMusic(
    backend({ likedIds: () => (++count === 2 ? old.promise : Promise.resolve(data([]))) }),
  );
  await music.bootstrap();
  const refresh = music.refreshLikes();
  await music.like(song);
  old.resolve(data([]));
  await refresh;
  assert.equal(music.liked.value.has(song.id), true);
  const stale = deferred<AccountData<string[]>>();
  const other = useMusic(backend({ likedIds: () => stale.promise }));
  other.account.value = { epoch: '2', status: 'authenticated', nickname: null, avatarUrl: null };
  const pending = other.refreshLikes();
  stale.resolve(data(['1'], '1'));
  await pending;
  assert.equal(other.liked.value.size, 0);
});

test('an old native like refresh failure cannot expire the replacement account', async () => {
  const stale = deferred<AccountData<string[]>>();
  const music = useMusic(backend({ likedIds: () => stale.promise }));
  music.account.value = { epoch: '1', status: 'authenticated', nickname: null, avatarUrl: null };
  const pending = music.refreshLikes();
  music.account.value = { epoch: '2', status: 'authenticated', nickname: null, avatarUrl: null };
  stale.reject({ code: 'session_expired', message: 'old account expired' });
  await pending;
  assert.equal(music.account.value.epoch, '2');
  assert.equal(music.account.value.status, 'authenticated');
});
