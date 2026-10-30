import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSearchResults } from '../src/composables/useSearchResults.ts';
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
