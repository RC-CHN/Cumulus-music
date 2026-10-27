import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useFmDiscovery } from '../src/composables/useFmDiscovery.ts';
import type { FmClient, FmDiscovery, FmDiscoveryQuery } from '../src/services/fm.ts';
import type { AccountData } from '../src/services/contracts.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: 'test',
  avatarUrl: null,
});
function result(query: FmDiscoveryQuery): FmDiscovery {
  return {
    query,
    fetchedAt: 1,
    page: { cards: [], zones: [], tabs: [], activeTab: null, defaultFallback: false },
  };
}
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { resolve, promise };
}
test('FM专区快速切换和账号切换拒绝迟到的目录', async () => {
  const late = deferred<AccountData<FmDiscovery>>();
  const core = {
    fmDiscovery: async (epoch: string, q: FmDiscoveryQuery, refresh: boolean) => {
      if (!refresh) throw Error('cache missing');
      return q.kind === 'zone' ? late.promise : { epoch, value: result(q) };
    },
  } as FmClient;
  const m = useFmDiscovery(core, () => {});
  m.connect(account());
  const old = m.load({ kind: 'zone', page_code: 'a', tab_key: 'default' });
  await Promise.resolve();
  await m.load({ kind: 'home' });
  late.resolve({ epoch: '1', value: result({ kind: 'zone', page_code: 'a', tab_key: 'default' }) });
  await old;
  assert.equal(m.page.value?.query.kind, 'home');
  m.connect(account('2'));
  assert.equal(m.home.value, undefined);
  assert.equal(m.page.value, undefined);
  m.dispose();
});
test('FM目录保留明确的缓存供离线查看并显示刷新失败', async () => {
  const core = {
    fmDiscovery: async (epoch: string, q: FmDiscoveryQuery, refresh: boolean) => {
      if (refresh) throw { code: 'network', message: 'offline' };
      return { epoch, value: result(q) };
    },
  } as FmClient;
  const m = useFmDiscovery(core, () => {});
  m.connect(account());
  await m.load();
  assert.equal(m.page.value?.fetchedAt, 1);
  assert.ok(m.error.value);
  assert.equal(m.loading.value, false);
  m.dispose();
});
