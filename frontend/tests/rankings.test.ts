import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useRankings } from '../src/composables/useRankings.ts';
import { movement, type RankingView } from '../src/services/rankings.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'fixture',
  avatarUrl: null,
};
const view = (fresh = true): RankingView => ({
  fetchedAt: 1,
  cached: true,
  fresh,
  page: { kind: 'home', groups: [] },
});
const reply = (v = view(), epoch = '1') => ({ epoch, value: v });
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
test('repeated navigation to an in-flight page preserves the original result', async () => {
  const late = deferred<ReturnType<typeof reply>>();
  let calls = 0;
  const m = useRankings({
    rankingPage: async () => {
      calls++;
      return late.promise;
    },
  });
  m.connect(account);
  const first = m.open({ kind: 'home' });
  await m.open({ kind: 'home' });
  late.resolve(reply());
  await first;
  assert.equal(calls, 1);
  assert.equal(m.view.value?.page.kind, 'home');
  assert.equal(m.loading.value, false);
});
test('official fresh cache avoids a remote request; explicit refresh bypasses it', async () => {
  const calls: boolean[] = [];
  const m = useRankings({
    rankingPage: async (_e, _q, refresh) => {
      calls.push(refresh);
      return reply();
    },
  });
  m.connect(account);
  await m.open({ kind: 'home' });
  assert.deepEqual(calls, [false]);
  await m.refresh();
  assert.deepEqual(calls, [false, true]);
  assert.equal(m.loading.value, false);
});
test('expired cache survives a failed refresh and explicit retry replaces it', async () => {
  let fail = true;
  const m = useRankings({
    rankingPage: async (_e, _q, refresh) => {
      if (refresh && fail) throw { code: 'network', message: '网络中断' };
      return reply(view(refresh));
    },
  });
  m.connect(account);
  await m.open({ kind: 'home' });
  assert.equal(m.view.value?.fresh, false);
  assert.equal(m.error.value, '网络中断');
  fail = false;
  await m.refresh();
  assert.equal(m.error.value, '');
  assert.equal(m.view.value?.fresh, true);
});
test('late query and old account responses cannot overwrite current ranking', async () => {
  const late = deferred<ReturnType<typeof reply>>();
  let n = 0;
  const m = useRankings({
    rankingPage: async () =>
      ++n === 1
        ? late.promise
        : reply({ ...view(), page: { kind: 'song_sales', generatedAt: 0, songs: [] } }),
  });
  m.connect(account);
  const first = m.open({ kind: 'home' });
  await m.open({ kind: 'song_sales' });
  late.resolve(reply());
  await first;
  assert.equal(m.view.value?.page.kind, 'song_sales');
  const old = deferred<ReturnType<typeof reply>>();
  const a = useRankings({ rankingPage: () => old.promise });
  a.connect(account);
  const pending = a.open({ kind: 'home' });
  a.connect({ ...account, epoch: '2' });
  old.resolve(reply());
  await pending;
  assert.equal(a.view.value, undefined);
  assert.equal(a.loading.value, false);
});
test('expired session is not disguised as a cache miss or retried online', async () => {
  let calls = 0,
    expired = 0;
  const m = useRankings(
    {
      rankingPage: async () => {
        calls++;
        throw { code: 'session_expired', message: '登录已失效' };
      },
    },
    () => expired++,
  );
  m.connect(account);
  await m.open({ kind: 'home' });
  assert.equal(calls, 1);
  assert.equal(expired, 1);
  assert.equal(m.view.value, undefined);
});
test('official preview movement distinguishes a new song from sales rows', () => {
  const r = { id: '1', name: '', artist: '', coverUrl: null, rank: 1, lastRank: 0, isAlbum: false };
  assert.equal(movement(r), '新');
  assert.equal(movement({ ...r, isAlbum: true }), '↓');
  assert.equal(movement({ ...r, lastRank: 2 }), '↑');
  assert.equal(movement({ ...r, lastRank: 1 }), '—');
  assert.equal(movement({ ...r, rank: null, lastRank: null }), '—');
});
