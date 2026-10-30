import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSearchBox } from '../src/composables/useSearchBox.ts';
import type { CoreClient, AccountData } from '../src/services/contracts.ts';
import type { SearchClient, SearchDiscovery } from '../src/services/search.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
const data = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (v: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
function fixture(overrides: Partial<SearchClient> = {}) {
  const core: SearchClient = {
    searchHistory: async () => data([]),
    searchDiscovery: async (_e, q) =>
      data(
        q.kind === 'defaults'
          ? {
              kind: 'defaults',
              words: [{ keyword: '实际默认词', label: '默认显示', tag: null }],
              refreshMs: 2500,
            }
          : { kind: 'charts', charts: [] },
      ),
    ...overrides,
  };
  return useSearchBox(core as CoreClient);
}
test('composing input does not request suggestions or submit before completion', async () => {
  const calls: string[] = [];
  const m = fixture({
    searchDiscovery: async (e, q) => {
      calls.push(q.kind);
      return data({ kind: 'defaults', words: [], refreshMs: 2500 }, e);
    },
  });
  await m.connect(account());
  m.show();
  m.composition(true);
  m.input('zhong');
  assert.equal(m.submit(), undefined);
  await new Promise((r) => setTimeout(r, 230));
  assert.ok(!calls.includes('suggestions'));
  m.query.value = '中文';
  m.composition(false);
  await new Promise((r) => setTimeout(r, 230));
  assert.equal(calls.filter((c) => c === 'suggestions').length, 1);
  m.dispose();
});
test('closed search ignores late suggestions and account switches clear search history', async () => {
  const pending = deferred<AccountData<SearchDiscovery>>();
  const m = fixture({
    searchDiscovery: (e, q) =>
      q.kind === 'suggestions'
        ? pending.promise
        : Promise.resolve(data({ kind: 'defaults', words: [], refreshMs: 2500 }, e)),
    searchHistory: async (e) => data(e === '1' ? ['私有历史'] : [], e),
  });
  await m.connect(account());
  m.show();
  m.query.value = '旧';
  const read = m.load({ kind: 'suggestions', keyword: '旧' });
  m.close();
  pending.resolve(
    data({
      kind: 'suggestions',
      title: '旧',
      recommended: [],
      suggestions: [{ keyword: '旧', label: '旧', tag: null }],
    }),
  );
  await read;
  assert.equal(m.open.value, false);
  assert.deepEqual(m.suggestions.value, []);
  await m.connect(account('2'));
  assert.deepEqual(m.history.value, []);
  m.dispose();
});
test('history failure never claims a clear succeeded and a default search uses realkeyword', async () => {
  const commands: unknown[] = [];
  const m = fixture({
    searchHistory: async (_e, c) => {
      commands.push(c);
      if (c.kind === 'clear') throw { code: 'storage', message: '无法保存搜索历史' };
      return data(['已有历史']);
    },
  });
  await m.connect(account());
  await m.historyCommand({ kind: 'clear' });
  assert.deepEqual(m.history.value, ['已有历史']);
  assert.match(m.errors.history!, /无法保存/);
  assert.equal(m.submit(), '实际默认词');
  await m.historyCommand({ kind: 'read' });
  assert.ok(commands.some((c) => (c as { keyword?: string }).keyword === '实际默认词'));
  m.dispose();
});
test('new query prevents an earlier success or failure from replacing current suggestions', async () => {
  const old = deferred<AccountData<SearchDiscovery>>();
  const m = fixture({
    searchDiscovery: (e, q) =>
      q.kind === 'suggestions' && q.keyword === 'old'
        ? old.promise
        : Promise.resolve(
            data(
              {
                kind: 'suggestions',
                title: '新',
                recommended: [],
                suggestions: [{ keyword: 'new', label: 'new', tag: null }],
              },
              e,
            ),
          ),
  });
  await m.connect(account());
  const read = m.load({ kind: 'suggestions', keyword: 'old' });
  await m.load({ kind: 'suggestions', keyword: 'new' });
  old.reject({ code: 'network', message: 'old failure' });
  await read;
  assert.equal(m.errors.suggestions, '');
  assert.equal(m.suggestions.value[0]?.keyword, 'new');
  m.dispose();
});
