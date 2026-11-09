import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useLocal } from '../src/composables/useLocal.ts';
import {
  defaultLocalQuery,
  localGroupQuery,
  type LocalClient,
  type LocalPage,
  type LocalQuery,
} from '../src/services/local.ts';
const turn = () => new Promise((r) => setImmediate(r));
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
function page(revision = 'a', total = 0): LocalPage {
  return {
    revision,
    total,
    playable: total,
    tracks: [],
    groups: [],
    roots: [],
    offset: 0,
    more: false,
  };
}
function fixture(overrides: Partial<LocalClient> = {}) {
  return {
    localPage: async () => page(),
    localCommand: async () => {},
    pickLocalDirectory: async () => null,
    openLocalTrackDirectory: async () => {},
    revealLocalFolder: async () => {},
    ...overrides,
  } satisfies LocalClient;
}
test('late page cannot overwrite a newer filter or reopen hidden polling', async () => {
  const delayed = deferred<LocalPage>();
  let calls = 0;
  const model = useLocal(
    fixture({ localPage: async () => (++calls === 1 ? delayed.promise : page('new', 3)) }),
  );
  model.show(true);
  model.filter({ keyword: 'chosen' });
  await turn();
  delayed.resolve(page('old', 8));
  await turn();
  assert.equal(model.view.value?.revision, 'new');
  assert.equal(model.query.value.keyword, 'chosen');
  model.show(false);
  model.dispose();
});
test('changed index resets pagination instead of combining incompatible snapshots', async () => {
  const queries: LocalQuery[] = [];
  const model = useLocal(
    fixture({
      localPage: async (q) => {
        queries.push(q);
        if (q.revision) throw { code: 'snapshot_changed', message: 'changed' };
        return page('new', 205);
      },
    }),
  );
  await model.refresh();
  model.page(200);
  await turn();
  await turn();
  assert.equal(model.query.value.offset, 0);
  assert.equal(model.query.value.revision, null);
  assert.equal(queries[1]?.revision, 'new');
  assert.equal(queries[2]?.offset, 0);
  assert.match(model.notice.value, /已更新/);
  model.dispose();
});
test('directory picker cancellation and disposal never enqueue a watch', async () => {
  const pending = deferred<string | null>();
  let writes = 0;
  const model = useLocal(
    fixture({
      pickLocalDirectory: () => pending.promise,
      localCommand: async () => {
        writes++;
      },
    }),
  );
  const operation = model.choose();
  model.dispose();
  pending.resolve('/fixture');
  await operation;
  assert.equal(writes, 0);
  const cancel = useLocal(
    fixture({
      localCommand: async () => {
        writes++;
      },
    }),
  );
  await cancel.choose();
  assert.equal(writes, 0);
  cancel.dispose();
});
test('a pending new query never exposes rows or groups from another query', async (t) => {
  const pending = deferred<LocalPage>();
  const model = useLocal(
    fixture({
      localPage: async (q) =>
        q.keyword === 'new' ? pending.promise : page(q.keyword || 'original', 3),
    }),
  );
  t.after(() => model.dispose());
  await model.refresh();
  assert.equal(model.view.value?.revision, 'original');
  model.filter({ keyword: 'new' });
  assert.equal(model.view.value, undefined);
  assert.equal(model.loading.value, true);
  pending.resolve(page('new', 1));
  await turn();
  assert.equal(model.view.value?.revision, 'new');
  model.query.value.sort = 'album';
  assert.equal(model.view.value, undefined, 'every presentation field must own its rows');
});

test('returning to the library keeps its committed query; grouping clears the old search and fences late replies', async (t) => {
  const pending = deferred<LocalPage>();
  const model = useLocal(
    fixture({
      localPage: async (q) => (q.keyword === 'late' ? pending.promise : page(q.group, 3)),
    }),
  );
  t.after(() => model.dispose());
  model.show(true);
  model.filter({ keyword: '006' });
  await turn();
  model.show(false);
  model.show(true);
  await turn();
  assert.equal(model.query.value.keyword, '006');
  model.filter({ keyword: 'late' });
  model.filter({ group: 'artist' });
  await turn();
  assert.equal(model.query.value.keyword, '');
  assert.equal(model.query.value.groupKey, null);
  assert.equal(model.view.value?.revision, 'artist');
  pending.resolve(page('late', 9));
  await turn();
  assert.equal(model.view.value?.revision, 'artist');
});

test('Back from a batch refreshes the ordinary list at its existing offset without an old revision', async (t) => {
  const reads: LocalQuery[] = [];
  let revision = 'before';
  const model = useLocal(
    fixture({
      localPage: async (query) => {
        reads.push(query);
        return { ...page(revision, 450), offset: query.offset };
      },
    }),
  );
  t.after(model.dispose);
  model.show(true);
  model.filter({ keyword: '合成', sort: 'album', group: 'artist', groupKey: 'singer' });
  await turn();
  model.page(200);
  await turn();
  assert.equal(reads.at(-1)?.revision, 'before');
  model.show(false);
  revision = 'after';
  model.show(true, true);
  await turn();
  assert.deepEqual(reads.at(-1), {
    ...defaultLocalQuery(),
    keyword: '合成',
    sort: 'album',
    group: 'artist',
    groupKey: 'singer',
    offset: 200,
    revision: null,
  });
  assert.equal(model.view.value?.revision, 'after');
});

test('refreshing native state clears only read errors and keeps an explicit failed command reviewable', async (t) => {
  let fail = false;
  const model = useLocal(
    fixture({
      localPage: async () => {
        if (fail) throw { code: 'io', message: '目录读取失败' };
        return page('stable', 3);
      },
      localCommand: async () => {
        throw { code: 'io', message: '目录暂不可用' };
      },
    }),
  );
  t.after(() => model.dispose());
  await model.refresh();
  await model.command({ kind: 'rescan' });
  assert.equal(model.error.value, '重新扫描：目录暂不可用');
  await model.refresh(true);
  assert.equal(model.error.value, '重新扫描：目录暂不可用');
  fail = true;
  await model.refresh(true);
  assert.equal(model.readError.value, '目录读取失败');
  assert.equal(model.view.value?.total, 3);
  fail = false;
  await model.refresh(true);
  assert.equal(model.readError.value, '');
  assert.equal(model.error.value, '重新扫描：目录暂不可用');
});

test('failed local commands release a superseded initial loading state and a late read cannot revive it', async (t) => {
  const pending = deferred<LocalPage>();
  let reads = 0;
  const model = useLocal(
    fixture({
      localPage: () => (++reads === 1 ? pending.promise : Promise.resolve(page('current', 2))),
      localCommand: async () => {
        throw { code: 'io', message: 'scan rejected' };
      },
    }),
  );
  t.after(() => model.dispose());
  const reading = model.refresh();
  assert(model.loading.value);
  await model.command({ kind: 'rescan' });
  assert(!model.loading.value);
  assert(!model.busy.value);
  pending.resolve(page('old', 9));
  await reading;
  assert.equal(model.view.value, undefined);
  await model.refresh();
  assert.equal(model.view.value?.revision, 'current');
  assert.match(model.error.value, /scan rejected/);
});

test('a context action uses the clicked native group without inheriting the selected page', () => {
  const query: LocalQuery = {
    ...defaultLocalQuery(),
    group: 'artist',
    groupKey: 'Elsewhere',
    keyword: 'Live',
    includeMissing: false,
    sort: 'duration',
    offset: 200,
    revision: 'page-fence',
  };
  const response = { ...page('index-fence'), groups: [{ key: '', name: '未知', count: 3 }] };
  const scope = localGroupQuery(query, response, '');
  assert.deepEqual(scope, { ...query, groupKey: '', offset: 0, revision: null });
  assert.equal(query.groupKey, 'Elsewhere');
  assert.equal(query.offset, 200);
  assert.equal(localGroupQuery(query, response, 'not-indexed'), null);
  assert.equal(localGroupQuery(defaultLocalQuery(), response, ''), null);
});

test('group list removal falls back only if the current group disappeared', async (t) => {
  let removed = false;
  const reads: Array<string | null> = [];
  const model = useLocal(
    fixture({
      localPage: async (query) => {
        reads.push(query.groupKey);
        const groups = [
          ...(!removed ? [{ key: 'Deleted', name: 'Deleted', count: 2 }] : []),
          { key: 'Remaining', name: 'Remaining', count: 1 },
        ];
        return { ...page(removed ? 'after' : 'before'), groups };
      },
      localCommand: async (command) => {
        assert.deepEqual(command, {
          kind: 'hide_group',
          query: { ...defaultLocalQuery(), group: 'album', groupKey: 'Deleted' },
          revision: 'before',
        });
        removed = true;
      },
    }),
  );
  t.after(() => model.dispose());
  const query = { ...defaultLocalQuery(), group: 'album' as const, groupKey: 'Deleted' };
  model.filter(query);
  await turn();
  await model.command({ kind: 'hide_group', query, revision: 'before' });
  assert.deepEqual(reads, ['Deleted', 'Deleted', 'Remaining']);
  assert.equal(model.query.value.groupKey, 'Remaining');
  assert.match(model.notice.value, /源文件仍保留/);
});
