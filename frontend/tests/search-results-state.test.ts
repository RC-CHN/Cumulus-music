import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSearchResults } from '../src/composables/useSearchResults.ts';
import { createSearchViewportMemory } from '../src/services/searchViewport.ts';
import type {
  SearchResultQuery,
  SearchResultView,
  SearchResultsClient,
} from '../src/services/search.ts';
import type { AccountData, AccountView } from '../src/services/contracts.ts';
const account: AccountView = {
  epoch: '1',
  status: 'authenticated',
  nickname: '合成账号',
  avatarUrl: null,
};
const query: SearchResultQuery = {
  keyword: '合成',
  category: 'all',
  offset: 0,
  limit: 30,
  correct: true,
};
const view = (q: SearchResultQuery): AccountData<SearchResultView> => ({
  epoch: '1',
  value: {
    query: q,
    sections: [],
    total: 0,
    more: false,
    nextOffset: q.offset,
    correction: null,
    unavailableCount: 0,
    notice: null,
  },
});
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
test('category and page changes reject late responses and disposal prevents resurrection', async () => {
  const old = deferred<AccountData<SearchResultView>>();
  let calls = 0;
  const api: SearchResultsClient = {
    searchResults: async (_e, q) => (++calls === 1 ? old.promise : view(q)),
  };
  const model = useSearchResults(api, () => assert.fail('no expiry'));
  model.connect(account);
  const first = model.open(query);
  const second = { ...query, category: 'artists' as const, offset: 12 };
  await model.open(second);
  old.resolve(view(query));
  await first;
  assert.deepEqual(model.view.value?.query, second);
  model.dispose();
  assert.equal(model.view.value, undefined);
  await model.open(query);
  assert.equal(calls, 2);
});
test('stale failures and mismatched account results cannot expire the new account', async () => {
  const old = deferred<AccountData<SearchResultView>>();
  let expired = 0,
    calls = 0;
  const model = useSearchResults(
    { searchResults: async () => (++calls === 1 ? old.promise : view(query)) },
    () => expired++,
  );
  model.connect(account);
  const first = model.open(query);
  model.connect({ ...account, epoch: '2' });
  await model.open(query);
  old.reject({ code: 'session_expired', message: '旧账号失效' });
  await first;
  assert.equal(model.view.value, undefined);
  assert.equal(model.error.value, '');
  assert.equal(expired, 0);
});
test('failure has a retry path and current expiry is forwarded', async () => {
  let mode = 'error',
    expired = 0;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) => {
        if (mode !== 'good')
          throw { code: mode === 'expired' ? 'session_expired' : 'transport', message: '请重试' };
        return view(q);
      },
    },
    () => expired++,
  );
  model.connect(account);
  await model.open(query);
  assert.equal(model.error.value, '请重试');
  assert.equal(model.loading.value, false);
  mode = 'good';
  await model.open(query);
  assert.equal(model.error.value, '');
  assert.equal(model.view.value?.total, 0);
  mode = 'expired';
  await model.open(query);
  assert.equal(expired, 1);
  assert.equal(model.view.value, undefined);
});

const artistQuery = { ...query, category: 'artists' as const, limit: 2 };
function artistPage(q: SearchResultQuery, ids: string[], more = true) {
  const result = view(q);
  result.value.sections = [
    {
      id: 'artists',
      title: '歌手',
      category: 'artists',
      items: ids.map((id) => ({
        kind: 'artist' as const,
        artist: { id, name: id, coverUrl: null, aliases: [], songCount: 3, albumCount: 1 },
      })),
    },
  ];
  result.value.more = more;
  result.value.nextOffset = q.offset + q.limit;
  return result;
}
test('continuous loading preserves the first records and retries the same cursor once', async () => {
  const calls: number[] = [];
  let fail = true;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) => {
        calls.push(q.offset);
        if (q.offset && fail) throw { code: 'transport', message: '请重试' };
        return artistPage(q, q.offset ? ['2', '3'] : ['1', '2'], !q.offset);
      },
    },
    () => assert.fail('expiry'),
  );
  model.connect(account);
  await model.open(artistQuery);
  model.savePosition({ top: 250, tracks: 0 });
  await model.more();
  assert.equal(model.moreError.value, '请重试');
  assert.equal(model.view.value?.sections[0]?.items.length, 2);
  assert.equal(model.position.value.top, 250);
  fail = false;
  await model.more();
  assert.deepEqual(calls, [0, 2, 2]);
  assert.equal(model.view.value?.sections[0]?.items.length, 3);
  assert.equal(model.view.value?.more, false);
  assert.equal(model.moreError.value, '');
});
test('in-flight append is coalesced and its old-account error cannot expire a new search', async () => {
  const pending = deferred<AccountData<SearchResultView>>();
  let appends = 0;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) =>
        q.offset ? (appends++, pending.promise) : artistPage(q, ['1', '2']),
    },
    () => assert.fail('late expiry'),
  );
  model.connect(account);
  await model.open(artistQuery);
  const first = model.more();
  await model.more();
  assert.equal(appends, 1);
  model.connect({ ...account, epoch: '2' });
  pending.reject({ code: 'session_expired', message: '旧账号' });
  await first;
  assert.equal(model.moreError.value, '');
  assert.equal(model.view.value, undefined);
});
test('repeated pages stop automatic loading without losing visible records', async () => {
  const model = useSearchResults(
    { searchResults: async (_e, q) => artistPage(q, ['1', '2']) },
    () => {},
  );
  model.connect(account);
  await model.open(artistQuery);
  await model.more();
  assert.equal(model.view.value?.sections[0]?.items.length, 2);
  assert.equal(model.view.value?.more, false);
  assert.match(model.view.value!.notice!, /没有更多/);
});
test('return navigation restores account-owned results, scroll and retry state; new searches refresh', async () => {
  const memory = createSearchViewportMemory();
  let calls = 0;
  const api: SearchResultsClient = {
    searchResults: async (_e, q) => {
      calls++;
      if (q.offset) throw { code: 'transport', message: '续页失败' };
      return artistPage(q, ['1', '2']);
    },
  };
  const first = useSearchResults(api, () => {}, memory);
  first.connect(account);
  await first.open(artistQuery, true, 1);
  await first.more();
  first.savePosition({ top: 400, tracks: 0, focus: 'artist:2' });
  first.dispose();
  const second = useSearchResults(api, () => {}, memory);
  second.connect(account);
  await second.open(artistQuery, true, 1);
  assert.equal(calls, 2);
  assert.deepEqual(second.position.value, { top: 400, tracks: 0, focus: 'artist:2' });
  assert.equal(second.moreError.value, '续页失败');
  await second.open(artistQuery, true, 2);
  assert.equal(calls, 3);
  assert.equal(second.position.value.top, 0);
  second.connect({ ...account, status: 'expired' });
  second.connect(account);
  await second.open(artistQuery, true, 1);
  assert.equal(calls, 4);
});
test('browsing memory evicts old visits and never accepts a non-advancing cursor', async () => {
  const memory = createSearchViewportMemory(2);
  let calls = 0;
  const model = useSearchResults(
    {
      searchResults: async (_e, q) => {
        calls++;
        const result = artistPage(q, ['1', '2']);
        if (q.offset) result.value.nextOffset = q.offset;
        return result;
      },
    },
    () => {},
    memory,
  );
  model.connect(account);
  for (const session of [1, 2, 3, 1]) await model.open(artistQuery, true, session);
  assert.equal(calls, 4);
  await model.more();
  assert.equal(model.view.value?.sections[0]?.items.length, 2);
  assert.match(model.moreError.value, /未更新/);
});
test('unmounting an old account page cannot repopulate the replacement account memory', async () => {
  const memory = createSearchViewportMemory();
  const old = useSearchResults(
    { searchResults: async (_e, q) => artistPage(q, ['1']) },
    () => {},
    memory,
  );
  old.connect(account);
  await old.open(artistQuery, true, 1);
  memory.connect({ ...account, epoch: '2' });
  old.savePosition({ top: 100, tracks: 0 });
  assert.equal(memory.get(memory.key(artistQuery, 1), '2'), undefined);
});
