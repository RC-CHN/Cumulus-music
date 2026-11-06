import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope } from 'vue';
import { useLocal } from '../src/composables/useLocal.ts';
import { useLocalSearch } from '../src/composables/useLocalSearch.ts';
import type { LocalQuery, LocalPage } from '../src/services/local.ts';
const turn = () => new Promise((resolve) => setImmediate(resolve));
const settle = () => new Promise((resolve) => setTimeout(resolve, 25));
function fixture() {
  const reads: LocalQuery[] = [];
  const model = useLocal({
    localPage: async (q) => {
      reads.push({ ...q });
      return {
        revision: 'native',
        tracks: [],
        groups: [],
        roots: [],
        total: 0,
        playable: 0,
        offset: q.offset,
        more: false,
      } satisfies LocalPage;
    },
    localCommand: async () => {},
    pickLocalDirectory: async () => null,
    openLocalTrackDirectory: async () => {},
    revealLocalFolder: async () => {},
  });
  model.show(true);
  const scope = effectScope();
  const search = scope.run(() => useLocalSearch(model))!;
  return {
    model,
    scope,
    search,
    reads,
    dispose() {
      scope.stop();
      model.dispose();
    },
  };
}
test('official debounced search preserves spaces and flushes pending input before leaving a page', async (t) => {
  const f = fixture();
  t.after(() => f.dispose());
  await turn();
  f.search.input('Old ');
  await settle();
  assert.equal(f.model.query.value.keyword, 'Old ');
  assert.equal(f.search.keyword.value, 'Old ');
  f.search.input('Old title');
  f.search.flush();
  f.model.show(false);
  await turn();
  assert.equal(f.model.query.value.keyword, 'Old title');
  f.model.show(true);
  const scope = effectScope();
  t.after(() => scope.stop());
  const returned = scope.run(() => useLocalSearch(f.model))!;
  assert.equal(returned.keyword.value, 'Old title');
});
test('IME input does not filter partial composition and commits the completed text once', async (t) => {
  const f = fixture();
  t.after(() => f.dispose());
  await turn();
  const before = f.reads.length;
  f.search.compositionStart();
  f.search.input('zhong');
  f.search.flush();
  await settle();
  assert.equal(f.reads.length, before);
  f.search.compositionEnd('中文');
  await turn();
  assert.equal(f.model.query.value.keyword, '中文');
  assert.equal(f.reads.length, before + 1);
});
test('group changes clear pending text and timers from hidden or disposed visits cannot overwrite a new visit', async (t) => {
  const f = fixture();
  t.after(() => f.dispose());
  await turn();
  f.search.input('pending');
  f.search.group('artist');
  await settle();
  assert.equal(f.model.query.value.keyword, '');
  assert.equal(f.model.query.value.group, 'artist');
  f.search.input('old owner');
  f.model.show(false);
  f.model.show(true);
  const scope = effectScope();
  t.after(() => scope.stop());
  const next = scope.run(() => useLocalSearch(f.model))!;
  next.input('current owner');
  next.flush();
  await settle();
  f.search.flush();
  assert.equal(f.model.query.value.keyword, 'current owner');
  next.input('discarded');
  scope.stop();
  await settle();
  assert.equal(f.model.query.value.keyword, 'current owner');
});
test('clearing a search cancels its queued filter and survives external category resets during composition', async (t) => {
  const f = fixture();
  t.after(() => f.dispose());
  await turn();
  f.search.input('006');
  f.search.clear();
  await settle();
  assert.equal(f.search.keyword.value, '');
  assert.equal(f.model.query.value.keyword, '');
  f.search.compositionStart();
  f.model.filter({ group: 'folder' });
  assert.equal(f.search.keyword.value, '');
  f.search.input('new folder');
  f.search.flush();
  await turn();
  assert.equal(f.model.query.value.keyword, 'new folder');
});
test('sorting folds a pending search into one native read without sending partial IME input', async (t) => {
  const f = fixture();
  t.after(() => f.dispose());
  await turn();
  const before = f.reads.length;
  f.search.input('夜行 ');
  f.search.refine({ sort: 'duration', descending: true });
  await settle();
  assert.equal(f.reads.length, before + 1);
  assert.deepEqual(
    [f.reads.at(-1)?.keyword, f.reads.at(-1)?.sort, f.reads.at(-1)?.descending],
    ['夜行 ', 'duration', true],
  );
  f.search.compositionStart();
  f.search.input('yu');
  f.search.refine({ sort: 'title', descending: false });
  assert.equal(f.reads.at(-1)?.keyword, '夜行 ');
  f.search.compositionEnd('雨声');
  await turn();
  assert.deepEqual([f.reads.at(-1)?.keyword, f.reads.at(-1)?.sort], ['雨声', 'title']);
});
test('selecting a named group or changing availability folds a pending search into the same read', async (t) => {
  const f = fixture();
  t.after(() => f.dispose());
  f.search.group('artist');
  await turn();
  const before = f.reads.length;
  f.search.input('归途');
  f.search.refine({ groupKey: '上海' });
  await settle();
  assert.equal(f.reads.length, before + 1);
  assert.deepEqual([f.reads.at(-1)?.keyword, f.reads.at(-1)?.groupKey], ['归途', '上海']);
  f.search.input('其他');
  f.search.refine({ includeMissing: true });
  await settle();
  assert.equal(f.reads.length, before + 2);
  assert.deepEqual(
    [f.reads.at(-1)?.keyword, f.reads.at(-1)?.groupKey, f.reads.at(-1)?.includeMissing],
    ['其他', '上海', true],
  );
});
