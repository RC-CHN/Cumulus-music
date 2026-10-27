import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSavedAccounts } from '../src/composables/useSavedAccounts.ts';
import type { SavedAccount } from '../src/services/auth.ts';
const item: SavedAccount = {
  id: 'saved',
  nickname: 'fixture',
  avatarUrl: null,
  current: false,
  autoLogin: true,
};
const page = (value: SavedAccount[]) => ({ epoch: '1', value });
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
test('late list read cannot resurrect a removed account', async () => {
  const late = deferred<ReturnType<typeof page>>();
  let reads = 0;
  const m = useSavedAccounts(
    {
      savedAccounts: async () => (++reads === 1 ? late.promise : page([])),
      removeSavedAccount: async () => {},
    },
    '1',
  );
  const before = m.load();
  assert.equal(await m.remove(item), true);
  late.resolve(page([item]));
  await before;
  assert.deepEqual(m.accounts.value, []);
  assert.equal(m.loading.value, false);
});
test('lost removal reply reads back absence without replaying removal', async () => {
  let writes = 0;
  const m = useSavedAccounts(
    {
      savedAccounts: async () => page([]),
      removeSavedAccount: async () => {
        writes++;
        throw { code: 'network', message: 'IPC interrupted' };
      },
    },
    '1',
  );
  assert.equal(await m.remove(item), true);
  assert.equal(writes, 1);
  assert.equal(m.error.value, '');
});
test('failed cleanup stays visible for explicit retry and never removes current account', async () => {
  let writes = 0;
  const m = useSavedAccounts(
    {
      savedAccounts: async () => page([{ ...item, removalPending: true }]),
      removeSavedAccount: async () => {
        writes++;
        throw { code: 'storage', message: '无法清理' };
      },
    },
    '1',
  );
  assert.equal(await m.remove({ ...item, current: true }), false);
  assert.equal(writes, 0);
  assert.equal(await m.remove(item), false);
  assert.equal(writes, 1);
  assert.equal(m.accounts.value[0].removalPending, true);
  assert.equal(m.error.value, '无法清理');
  assert.equal(m.removing.value, false);
});
test('disposed account dialog does not read or publish a late removal result', async () => {
  const late = deferred<void>();
  let reads = 0;
  const m = useSavedAccounts(
    {
      savedAccounts: async () => {
        reads++;
        return page([]);
      },
      removeSavedAccount: () => late.promise,
    },
    '1',
  );
  const result = m.remove(item);
  m.dispose();
  late.resolve();
  assert.equal(await result, false);
  assert.equal(reads, 0);
});
