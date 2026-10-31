import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useDownloads } from '../src/composables/useDownloads.ts';
import { downloadViewportKey } from '../src/services/downloadViewport.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type {
  DownloadClient,
  DownloadResult,
  DownloadsView,
  DownloadTask,
} from '../src/services/downloads.ts';
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
const account = (epoch = '1'): AccountView => ({
  epoch,
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
});
function state(epoch = '1', total = 0, offset = 0): DownloadsView {
  return {
    epoch,
    total,
    offset,
    tasks: [],
    activeCount: 0,
    completedCount: total,
    directory: null,
    migration: null,
    options: { quality: 'standard', tags: true, cover: true, lyrics: false, container: 'audio' },
    operations: [],
    jobs: [],
  };
}
const turn = () => new Promise((r) => setImmediate(r));
test('a visit guard rejects old timers and returns preserve explicit continuation failure', async () => {
  const model = useDownloads({ downloads: async (e) => state(e) } as DownloadClient);
  await model.connect(account());
  const a = {},
    b = {};
  model.enterVisit(a);
  const owns = model.captureVisit();
  assert.equal(owns(), true);
  model.moreError.value = 'explicit retry required';
  model.leaveVisit();
  assert.equal(owns(), false);
  model.enterVisit(b);
  model.leaveVisit();
  model.enterVisit(a);
  assert.equal(model.moreError.value, 'explicit retry required');
  await model.connect(account('2'));
  assert.equal(owns(), false);
  model.dispose();
});
test('refreshing the same authenticated identity retains its browsing visits', async () => {
  const model = useDownloads({ downloads: async (e) => state(e) } as DownloadClient);
  await model.connect(account());
  const a = {},
    b = {};
  model.enterVisit(a);
  await model.filter({ category: 'voice', keyword: 'same-account' });
  model.query.value.limit = 300;
  model.leaveVisit();
  model.enterVisit(b, { tab: 'active' });
  await model.connect(account());
  model.leaveVisit();
  model.enterVisit(a);
  assert.equal(model.query.value.keyword, 'same-account');
  assert.equal(model.query.value.category, 'voice');
  assert.equal(model.query.value.limit, 300);
  model.dispose();
});
test('download visits retain independent queries, loaded prefixes and positions across nested navigation', async () => {
  const model = useDownloads({ downloads: async (e) => state(e) } as DownloadClient);
  await model.connect(account());
  const a = {},
    b = {};
  model.enterVisit(a);
  await model.filter({ keyword: 'song-A' });
  model.query.value.limit = 300;
  const saved = {
    epoch: '1',
    query: downloadViewportKey(model.query.value),
    anchor: 'a',
    index: 180,
    offset: 9,
    growth: [1, 2],
  };
  model.saveViewport(saved, a);
  model.leaveVisit();
  model.enterVisit(b, { tab: 'active', keyword: '' });
  assert.equal(model.query.value.tab, 'active');
  assert.equal(model.viewport.value, undefined);
  await model.filter({ tab: 'completed', category: 'voice', keyword: 'voice-B' });
  model.leaveVisit();
  model.enterVisit(a);
  assert.equal(model.query.value.keyword, 'song-A');
  assert.equal(model.query.value.limit, 300);
  assert.equal(model.viewport.value, saved);
  model.leaveVisit();
  model.enterVisit(b, { tab: 'active' });
  assert.equal(model.query.value.category, 'voice');
  assert.equal(model.query.value.keyword, 'voice-B');
  model.dispose();
});
test('late callbacks from another visit cannot replace the new read or save its viewport', async () => {
  const pending = deferred<DownloadsView>();
  const model = useDownloads({
    downloads: async (e, q) => (q.keyword === 'held' ? pending.promise : state(e, 17)),
  } as DownloadClient);
  await model.connect(account());
  const a = {},
    b = {};
  model.enterVisit(a);
  const saved = {
    epoch: '1',
    query: downloadViewportKey(model.query.value),
    anchor: 'old',
    index: 0,
    offset: 0,
    growth: [],
  };
  const held = model.filter({ keyword: 'held' });
  model.leaveVisit();
  model.enterVisit(b, { keyword: '' });
  await model.refresh();
  model.saveViewport(saved, a);
  assert.equal(model.viewport.value, undefined);
  pending.resolve(state('1', 999));
  await held;
  assert.equal(model.view.value?.total, 17);
  model.dispose();
});
test('account lifecycle clears browsing memory and query without reviving another account snapshots', async () => {
  const model = useDownloads({ downloads: async (e) => state(e) } as DownloadClient);
  await model.connect(account());
  const a = {},
    b = {};
  model.enterVisit(a);
  await model.filter({ category: 'mv', keyword: 'old-account' });
  model.leaveVisit();
  model.enterVisit(b);
  await model.connect(account('2'));
  model.leaveVisit();
  model.enterVisit(a);
  assert.equal(model.query.value.keyword, '');
  assert.equal(model.query.value.category, 'song');
  assert.equal(model.viewport.value, undefined);
  model.dispose();
  assert.equal(model.visit.value, undefined);
});
test('download return location survives reads but rejects another query and late account callbacks', async () => {
  const model = useDownloads({ downloads: async (epoch) => state(epoch) } as DownloadClient);
  await model.connect(account());
  const saved = {
    epoch: '1',
    query: downloadViewportKey(model.query.value),
    anchor: 'download-one',
    index: 80,
    offset: 7,
    focus: { task: 'download-one' },
    growth: [1, 2],
  };
  model.saveViewport(saved);
  await model.refresh(true);
  assert.equal(model.viewport.value, saved);
  await model.filter({ keyword: 'changed' });
  assert.equal(model.viewport.value, undefined);
  model.saveViewport(saved);
  assert.equal(model.viewport.value, undefined);
  await model.filter({ keyword: '' });
  model.saveViewport(saved);
  await model.connect(account('2'));
  assert.equal(model.viewport.value, undefined);
  model.saveViewport({ ...saved, query: downloadViewportKey(model.query.value) });
  assert.equal(model.viewport.value, undefined);
  model.dispose();
  model.saveViewport({ ...saved, epoch: '2' });
  assert.equal(model.viewport.value, undefined);
});
test('download account switch discards late reads, writes and native picker selections', async () => {
  const first = deferred<DownloadsView>(),
    write = deferred<DownloadResult>(),
    picked = deferred<string | null>();
  const core = {
    downloads: async (e: string) => (e === '1' ? first.promise : state(e)),
    downloadCommand: () => write.promise,
    pickDownloadDirectory: () => picked.promise,
  } as DownloadClient;
  const model = useDownloads(core);
  const old = model.connect(account());
  const pending = model.command({ kind: 'all', action: 'pause' });
  const picker = model.pick();
  await model.connect(account('2'));
  first.resolve(state('1', 50));
  write.resolve({ epoch: '1', accepted: ['old'], failed: [], operationId: null });
  picked.resolve('/old-account');
  await old;
  assert.equal(await pending, undefined);
  assert.equal(await picker, undefined);
  assert.equal(model.view.value?.epoch, '2');
  assert.equal(model.view.value?.total, 0);
  assert.equal(model.notice.value, '');
  assert.equal(model.busy.value, false);
  model.dispose();
});
test('newer download query wins and resets a growing window to its initial prefix', async () => {
  const old = deferred<DownloadsView>();
  const limits: number[] = [];
  const model = useDownloads({
    downloads: async (e, q) => {
      limits.push(q.limit);
      if (q.keyword === 'old') return old.promise;
      return state(e, q.keyword === 'new' ? 101 : 0, q.offset);
    },
  } as DownloadClient);
  await model.connect(account());
  model.filter({ keyword: 'old' });
  model.filter({ keyword: 'new', offset: 200, limit: 300 });
  await turn();
  assert.equal(model.query.value.offset, 0);
  assert.equal(model.query.value.limit, 100);
  assert.equal(model.view.value?.total, 101);
  old.resolve(state('1', 999));
  await turn();
  assert.equal(model.view.value?.total, 101);
  assert.deepEqual(limits, [100, 100, 100]);
  model.dispose();
});
test('download write error survives successful polling and batch partial success stays visible', async () => {
  let failing = true;
  const model = useDownloads({
    downloads: async (e) => state(e),
    downloadCommand: async (e) => {
      if (failing) throw { code: 'state', message: '目录冲突', retryable: false };
      return {
        epoch: e,
        accepted: ['one'],
        failed: [{ id: 'two', message: '未完成' }],
        operationId: null,
      };
    },
  } as DownloadClient);
  await model.connect(account());
  await model.command({ kind: 'all', action: 'pause' });
  await model.refresh(true);
  assert.equal(model.error.value, '暂停全部下载：目录冲突');
  failing = false;
  const result = await model.command({ kind: 'all', action: 'pause' });
  assert.equal(result?.failed.length, 1);
  assert.match(model.notice.value, /1 项未完成/);
  assert.equal(model.error.value, '');
  model.dispose();
});
test('closing download page or disposing model never cancels a native transfer', async () => {
  let writes = 0;
  const result = deferred<DownloadsView>();
  const model = useDownloads({
    downloads: () => result.promise,
    downloadCommand: async () => {
      writes++;
      throw Error('unexpected');
    },
  } as DownloadClient);
  const pending = model.connect(account());
  model.show(false);
  model.dispose();
  result.resolve(state('1', 42));
  await pending;
  assert.equal(model.view.value, undefined);
  assert.equal(writes, 0);
});
test('switching download category hides old rows until the matching native query finishes', async () => {
  const pending = deferred<DownloadsView>();
  const seen: string[] = [];
  const model = useDownloads({
    downloads: async (epoch, query) => {
      seen.push(`${query.tab}:${query.category}`);
      return query.category === 'voice' ? pending.promise : state(epoch, 120);
    },
  } as DownloadClient);
  await model.connect(account());
  assert.equal(model.view.value?.total, 120);
  const reading = model.filter({ category: 'voice' });
  assert.equal(model.view.value, undefined);
  assert.equal(model.loading.value, true);
  await model.filter({ tab: 'active' });
  pending.resolve(state('1', 12));
  await reading;
  assert.equal(model.query.value.category, 'all');
  assert.equal(model.view.value?.total, 120);
  assert.deepEqual(seen, ['completed:song', 'completed:voice', 'active:all']);
  model.dispose();
});
function windowState(epoch: string, ids: number[], total: number): DownloadsView {
  return { ...state(epoch, total), tasks: ids.map((id) => ({ id: String(id) }) as DownloadTask) };
}
test('continuous download growth replaces a consistent prefix and preserves rows on failure', async () => {
  let broken = false;
  let ids = Array.from({ length: 240 }, (_, i) => i + 1);
  const calls: number[] = [];
  const model = useDownloads({
    downloads: async (epoch, query) => {
      calls.push(query.limit);
      if (broken) throw { code: 'state', message: '继续读取失败' };
      return windowState(epoch, ids.slice(0, query.limit), ids.length);
    },
  } as DownloadClient);
  await model.connect(account());
  assert.equal(model.view.value?.tasks.length, 100);
  broken = true;
  await model.loadMore();
  assert.equal(model.moreError.value, '继续读取失败');
  assert.equal(model.readError.value, '');
  assert.equal(model.query.value.limit, 100);
  assert.equal(model.view.value?.tasks.length, 100);
  broken = false;
  // A new task is sorted ahead of the existing first page during the failed read.
  ids = [999, ...ids];
  await model.loadMore();
  const loaded = model.view.value!.tasks.map((t) => t.id);
  assert.equal(loaded.length, 200);
  assert.equal(new Set(loaded).size, 200);
  assert.equal(loaded[0], '999');
  assert.equal(loaded[199], '199');
  await model.loadMore();
  assert.equal(model.view.value?.tasks.length, 241);
  assert.equal(model.hasMore.value, false);
  // Removal shrinks the same visible window; no stale empty final page survives.
  ids = ids.slice(0, 5);
  await model.refresh(true);
  assert.equal(model.view.value?.tasks.length, 5);
  assert.equal(model.hasMore.value, false);
  assert.deepEqual(calls, [100, 200, 200, 241, 241]);
  model.dispose();
});
test('category and account changes discard a pending growth read and concurrent growth is deduplicated', async () => {
  const pending = deferred<DownloadsView>();
  let growing = 0;
  const model = useDownloads({
    downloads: async (epoch, query) => {
      if (query.limit > 100) {
        growing++;
        return pending.promise;
      }
      return windowState(
        epoch,
        Array.from({ length: 100 }, (_, i) => i),
        500,
      );
    },
  } as DownloadClient);
  await model.connect(account());
  const first = model.loadMore();
  await model.loadMore();
  assert.equal(growing, 1);
  assert.equal(model.loadingMore.value, true);
  await model.filter({ category: 'voice' });
  assert.equal(model.loadingMore.value, false);
  await model.connect(account('2'));
  pending.resolve(windowState('1', [999], 999));
  await first;
  assert.equal(model.view.value?.epoch, '2');
  assert.equal(model.view.value?.tasks[0]?.id, '0');
  assert.equal(model.query.value.limit, 100);
  model.dispose();
});

test('download feedback retains operation context across category switches and late completion', async () => {
  const pending = deferred<DownloadResult>();
  const model = useDownloads({
    downloads: async (epoch) => state(epoch),
    downloadCommand: () => pending.promise,
  } as DownloadClient);
  await model.connect(account());
  const writing = model.command({ kind: 'all', action: 'pause' });
  await model.filter({ category: 'voice' });
  pending.resolve({ epoch: '1', accepted: ['one'], failed: [], operationId: null });
  await writing;
  assert.equal(model.notice.value, '暂停全部下载：已受理 1 项');
  await model.filter({ category: 'mv' });
  await model.refresh(true);
  assert.equal(model.notice.value, '暂停全部下载：已受理 1 项');
  model.dispose();
});

test('overlay visibility refresh retains loaded controls while its read is pending', async () => {
  let pending: ReturnType<typeof deferred<DownloadsView>> | undefined;
  const model = useDownloads({
    downloads: async (e) => (pending ? pending.promise : state(e)),
  } as DownloadClient);
  await model.connect(account());
  model.show(true);
  await turn();
  pending = deferred<DownloadsView>();
  const loaded = model.view.value;
  model.show(true);
  assert.equal(model.loading.value, false);
  assert.equal(model.view.value, loaded);
  pending.resolve(state());
  await turn();
  model.dispose();
});
