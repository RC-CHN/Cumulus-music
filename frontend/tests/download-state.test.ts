import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useDownloads } from '../src/composables/useDownloads.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type { DownloadClient, DownloadResult, DownloadsView } from '../src/services/downloads.ts';
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
test('newer download query wins and empty final page falls back to last valid page', async () => {
  const old = deferred<DownloadsView>();
  const offsets: number[] = [];
  const model = useDownloads({
    downloads: async (e, q) => {
      offsets.push(q.offset);
      if (q.keyword === 'old') return old.promise;
      return state(e, q.keyword === 'new' ? 101 : 0, q.offset);
    },
  } as DownloadClient);
  await model.connect(account());
  model.filter({ keyword: 'old' });
  model.filter({ keyword: 'new', offset: 200 });
  await turn();
  assert.equal(model.query.value.offset, 100);
  assert.equal(model.view.value?.total, 101);
  old.resolve(state('1', 999));
  await turn();
  assert.equal(model.view.value?.total, 101);
  assert.deepEqual(offsets.slice(-2), [200, 100]);
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
  assert.equal(model.error.value, '目录冲突');
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
