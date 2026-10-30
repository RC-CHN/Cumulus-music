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

test('repeating a search starts a fresh visit while category navigation and back keep its identity', async () => {
  const music = useMusic(backend());
  await music.bootstrap();
  music.navigate({ kind: 'search', keyword: 'same' });
  const first = music.page.value;
  assert.equal(first.kind, 'search');
  if (first.kind !== 'search') return;
  assert.ok(first.searchId);
  music.navigate({ ...first, category: 'artists' });
  assert.equal(music.page.value.kind === 'search' && music.page.value.searchId, first.searchId);
  music.navigate({ kind: 'search', keyword: 'same' });
  assert.notEqual(music.page.value.kind === 'search' && music.page.value.searchId, first.searchId);
  music.back();
  assert.equal(music.page.value.kind === 'search' && music.page.value.searchId, first.searchId);
});

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

test('a slow playlist read cannot overwrite a later page', async () => {
  const old = deferred<AccountData<TrackPage>>();
  const music = useMusic(
    backend({
      playlistPage: (id) =>
        id === 'old'
          ? old.promise
          : Promise.resolve(data({ ...page, tracks: [{ ...song, name: 'new' }] })),
    }),
  );
  await music.bootstrap();
  music.navigate({ kind: 'playlist', id: 'old' });
  music.navigate({ kind: 'playlist', id: 'new' });
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
      playlistPage: async (id) => {
        if (id === 'expired')
          throw { code: 'session_expired', message: '重新登录', retryable: false };
        return data(page, '0');
      },
    }),
  );
  await music.bootstrap();
  music.navigate({ kind: 'playlist', id: 'old-epoch' });
  await turn();
  assert.equal(music.result.value, undefined);
  music.navigate({ kind: 'playlist', id: 'expired' });
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

test('a late failed like cannot expire a replacement account or remove its busy marker', async () => {
  const old = deferred<AccountData<import('../src/services/contracts.ts').LikeResult>>();
  let epoch = '1';
  const music = useMusic(
    backend({
      accountSnapshot: async () => ({
        epoch,
        status: 'authenticated',
        nickname: 'new',
        avatarUrl: null,
      }),
      likedIds: async () => data([], epoch),
      playlists: async () => data([], epoch),
      dailySongs: async () => data(page, epoch),
      setLiked: () => old.promise,
    }),
  );
  await music.bootstrap();
  const writing = music.like(song);
  epoch = '2';
  await music.bootstrap(false);
  music.busyLikes.value = new Set(['1']);
  old.reject({ code: 'session_expired', message: 'old request' });
  await writing;
  assert.equal(music.signedIn.value, true);
  assert.equal(music.account.value.epoch, '2');
  assert.equal(music.busyLikes.value.has('1'), true);
  assert.equal(music.likeActivity.value, 0);
});

test('recovery clears only the like error while preserving unrelated page errors', async () => {
  const music = useMusic(
    backend({
      setLiked: async () => {
        throw { code: 'network', message: '喜欢请求回包丢失' };
      },
    }),
  );
  await music.bootstrap();
  music.error.value = '页面读取失败';
  music.notice.value = '歌单已更新';
  await music.like(song);
  assert.equal(music.likeNotice.value, '喜欢请求回包丢失');
  assert.equal(music.error.value, '页面读取失败');
  await music.refreshLikes();
  assert.equal(music.likeNotice.value, '');
  assert.equal(music.error.value, '页面读取失败');
  assert.equal(music.notice.value, '歌单已更新');
});

test('cancelled navigation keeps page/history and a confirmed departure runs only once', async () => {
  let leave: (() => void) | undefined;
  const music = useMusic(backend(), (commit) => {
    leave = commit;
  });
  await music.bootstrap();
  music.navigate({ kind: 'library' });
  assert.equal(music.page.value.kind, 'daily');
  assert.equal(music.historyLength.value, 0);
  leave!();
  leave!();
  await turn();
  assert.equal(music.page.value.kind, 'library');
  assert.equal(music.historyLength.value, 1);
  music.back();
  assert.equal(music.historyLength.value, 1);
  leave!();
  await turn();
  assert.equal(music.page.value.kind, 'daily');
  assert.equal(music.historyLength.value, 0);
});

test('an old leave confirmation cannot navigate after an account change or another departure', async () => {
  const leaves: (() => void)[] = [];
  const music = useMusic(backend(), (commit) => {
    leaves.push(commit);
  });
  await music.bootstrap();
  music.navigate({ kind: 'library' });
  music.replace({ kind: 'downloads' });
  leaves[1]!();
  leaves[0]!();
  await turn();
  assert.equal(music.page.value.kind, 'downloads');
  assert.equal(music.historyLength.value, 0);
  music.navigate({ kind: 'library' });
  music.account.value = { ...music.account.value, epoch: '2' };
  leaves[2]!();
  assert.equal(music.page.value.kind, 'downloads');
});

test('playlist refresh exposes waiting and keeps read failures until a successful retry', async () => {
  let fail = false;
  let pending: ReturnType<typeof deferred<AccountData<[]>>> | undefined;
  const music = useMusic(
    backend({
      playlists: async () => {
        if (pending) return pending.promise;
        if (fail) throw { code: 'transport', message: 'playlist unavailable' };
        return data([]);
      },
    }),
  );
  await music.bootstrap();
  fail = true;
  await music.resources();
  assert.equal(music.playlistError.value, 'playlist unavailable');
  assert.equal(music.resourcesLoading.value, false);
  pending = deferred<AccountData<[]>>();
  const retry = music.resources();
  assert.equal(music.resourcesLoading.value, true);
  assert.equal(music.playlistError.value, 'playlist unavailable');
  pending.resolve(data([]));
  await retry;
  assert.equal(music.resourcesLoading.value, false);
  assert.equal(music.playlistError.value, '');
});

test('old resource failures cannot replace the new account library or loading state', async () => {
  let epoch = '1';
  let pending: ReturnType<typeof deferred<AccountData<[]>>> | undefined;
  const music = useMusic(
    backend({
      accountSnapshot: async () => ({
        epoch,
        status: 'authenticated',
        nickname: 'synthetic',
        avatarUrl: null,
      }),
      playlists: async () => (pending ? pending.promise : data([], epoch)),
      likedIds: async () => data([], epoch),
    }),
  );
  await music.bootstrap();
  const stale = deferred<AccountData<[]>>();
  pending = stale;
  const before = music.resources();
  pending = undefined;
  epoch = '2';
  await music.bootstrap(false);
  stale.reject({ code: 'session_expired', message: 'previous account expired' });
  await before;
  assert.equal(music.account.value.epoch, '2');
  assert.equal(music.account.value.status, 'authenticated');
  assert.equal(music.resourcesLoading.value, false);
  assert.equal(music.playlistError.value, '');
});
