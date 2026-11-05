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
      suggestions: [
        { keyword: '旧', label: '旧', tag: null, labelParts: [{ text: '旧', match: true }] },
      ],
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
                suggestions: [
                  {
                    keyword: 'new',
                    label: 'new',
                    tag: null,
                    labelParts: [{ text: 'new', match: true }],
                  },
                ],
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

test('late native label parts cannot replace either current suggestion group', async () => {
  const previous = deferred<AccountData<SearchDiscovery>>();
  const ordinary = {
    keyword: '真实搜索词',
    label: '🎧New <live>',
    tag: null,
    labelParts: [
      { text: '🎧', match: false },
      { text: 'New', match: true },
      { text: ' <live>', match: false },
    ],
  };
  const recommended = {
    keyword: '真实推荐词',
    label: 'New 城市',
    tag: '荐',
    labelParts: [
      { text: 'New', match: true },
      { text: ' 城市', match: false },
    ],
  };
  const m = fixture({
    searchDiscovery: async (epoch, query) => {
      if (query.kind !== 'suggestions')
        return data({ kind: 'defaults', words: [], refreshMs: 2500 }, epoch);
      if (query.keyword === 'old') return previous.promise;
      return data(
        {
          kind: 'suggestions',
          title: '新推荐',
          recommended: [recommended],
          suggestions: [ordinary],
        },
        epoch,
      );
    },
  });
  await m.connect(account());
  try {
    const old = m.load({ kind: 'suggestions', keyword: 'old' });
    await m.load({ kind: 'suggestions', keyword: 'new' });
    previous.resolve(
      data({
        kind: 'suggestions',
        title: '过期推荐',
        recommended: [],
        suggestions: [],
      }),
    );
    await old;
    assert.equal(m.suggestionTitle.value, '新推荐');
    assert.deepEqual(m.suggestions.value, [ordinary]);
    assert.deepEqual(m.recommended.value, [recommended]);
    assert.equal(m.pending.suggestions, false);
    assert.equal(m.errors.suggestions, '');
  } finally {
    m.dispose();
  }
});

test('a chosen label stays visible but history records only the real search term', async () => {
  const remembered: string[] = [];
  const m = fixture({
    searchHistory: async (_epoch, command) => {
      if (command.kind === 'remember') remembered.push(command.keyword);
      return data(remembered.slice());
    },
  });
  await m.connect(account());
  try {
    assert.equal(m.submit('实际关键词', ' 展示标签 '), '实际关键词');
    assert.equal(m.query.value, '展示标签');
    await m.historyCommand({ kind: 'read' });
    assert.deepEqual(remembered, ['实际关键词']);
    m.input('手动编辑');
    assert.equal(m.submit(), '手动编辑');
    assert.equal(m.query.value, '手动编辑');
  } finally {
    m.dispose();
  }
});

test('the search field distinguishes its rotating default from a typed keyword', async () => {
  const m = fixture();
  await m.connect(account());
  m.input('  ');
  assert.deepEqual(m.submitInput(), { keyword: '实际默认词', channel: 'defaultquery' });
  assert.equal(m.query.value, '实际默认词');
  m.input(' 实际默认词 ');
  assert.deepEqual(m.submitInput(), { keyword: '实际默认词', channel: 'typing' });
  m.composition(true);
  assert.equal(m.submitInput(), undefined);
  m.dispose();
});

test('empty submit uses the receipt of the exact rotating label, never one from a typed query', async (t) => {
  t.mock.timers.enable({ apis: ['setInterval', 'setTimeout'] });
  const m = fixture({
    searchDiscovery: async (epoch) =>
      data(
        {
          kind: 'defaults',
          words: [
            {
              keyword: '同一个词',
              label: '第一个展示词',
              tag: null,
              defaultReceipt: 'first-' + epoch,
            },
            {
              keyword: '同一个词',
              label: '第二个展示词',
              tag: null,
              defaultReceipt: 'second-' + epoch,
            },
          ],
          refreshMs: 2000,
        },
        epoch,
      ),
  });
  try {
    await m.connect(account());
    assert.equal(m.defaultWord.value?.label, '第一个展示词');
    t.mock.timers.tick(2000);
    assert.equal(m.defaultWord.value?.label, '第二个展示词');
    assert.deepEqual(m.submitInput(), {
      keyword: '同一个词',
      channel: 'defaultquery',
      defaultReceipt: 'second-1',
    });
    m.input('同一个词');
    assert.deepEqual(m.submitInput(), { keyword: '同一个词', channel: 'typing' });
    await m.connect(account('2'));
    assert.equal(m.submitInput()?.defaultReceipt, 'first-2');
  } finally {
    m.dispose();
    t.mock.timers.reset();
  }
});

test('discovery recommendations wait for latest local history without delaying charts', async () => {
  const initial = deferred<AccountData<string[]>>();
  const saving = deferred<AccountData<string[]>>();
  const contexts: string[] = [];
  let charts = 0;
  const m = fixture({
    searchHistory: (_epoch, command) =>
      command.kind === 'read' ? initial.promise : saving.promise,
    searchDiscovery: async (_epoch, query) => {
      if (query.kind === 'recommendations') contexts.push(query.lastQuery);
      if (query.kind === 'charts') charts++;
      return data({ kind: 'recommendations', words: [] });
    },
  });
  const connecting = m.connect(account());
  m.show();
  assert.deepEqual(contexts, []);
  assert.equal(charts, 1);
  initial.resolve(data(['初始历史']));
  await connecting;
  assert.deepEqual(contexts, ['初始历史']);

  m.submit('刚提交');
  m.show();
  m.input('');
  assert.deepEqual(contexts, ['初始历史']);
  saving.resolve(data(['刚提交', '初始历史']));
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(contexts, ['初始历史', '刚提交']);
  m.dispose();
});

test('a closed discovery panel does not send a late recommendation after history arrives', async () => {
  const initial = deferred<AccountData<string[]>>();
  const contexts: string[] = [];
  const m = fixture({
    searchHistory: () => initial.promise,
    searchDiscovery: async (_epoch, query) => {
      if (query.kind === 'recommendations') contexts.push(query.lastQuery);
      return data({ kind: 'recommendations', words: [] });
    },
  });
  const connecting = m.connect(account());
  m.show();
  m.close();
  initial.resolve(data(['未开启浮层']));
  await connecting;
  assert.deepEqual(contexts, []);
  m.show();
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(contexts, ['未开启浮层']);
  m.dispose();
});
