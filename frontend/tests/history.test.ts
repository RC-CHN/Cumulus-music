import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useHistory } from '../src/composables/useHistory.ts';
import {
  historyTime,
  historyOutcome,
  type HistoryClient,
  type HistoryCommand,
  type HistoryKind,
  type HistoryPage,
} from '../src/services/history.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: '合成账号',
  avatarUrl: null,
};
function page(kind: HistoryKind = 'song'): HistoryPage {
  return {
    kind,
    revision: 'a'.repeat(64),
    syncEnabled: true,
    canSync: true,
    remoteIncluded: true,
    remoteFetchedAt: 1,
    localCount: 0,
    entries: [],
    operations: [],
  };
}
function reply(value = page(), epoch = '1') {
  return { epoch, value: structuredClone(value) };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
const intent = (): Extract<HistoryCommand, { kind: 'mutate' }> => ({
  kind: 'mutate',
  id: '00000000-0000-0000-0000-000000000001',
  expected_revision: 'a'.repeat(64),
  action: { kind: 'clear' },
});
function backend(overrides: Partial<HistoryClient> = {}): HistoryClient {
  return {
    historyPage: async (e, k) => reply(page(k), e),
    historyCommand: async (e, k) => reply(page(k), e),
    ...overrides,
  };
}
test('cached history is visible while refreshing, and survives a network failure', async () => {
  const remote = deferred<ReturnType<typeof reply>>();
  let fail = false;
  const calls: boolean[] = [];
  const model = useHistory(
    backend({
      historyPage: async (_e, k, refresh) => {
        calls.push(refresh);
        if (refresh && fail) throw { code: 'network', message: '离线' };
        return refresh ? remote.promise : reply(page(k));
      },
    }),
  );
  model.connect(account);
  const loading = model.open('song');
  await new Promise((r) => setImmediate(r));
  assert.equal(model.page.value?.kind, 'song');
  assert.equal(model.loading.value, true);
  assert.equal(model.canMutate.value, false);
  remote.resolve(reply());
  await loading;
  fail = true;
  await model.refresh();
  assert.equal(model.page.value?.kind, 'song');
  assert.equal(model.error.value, '离线');
  assert.deepEqual(calls, [false, true, true]);
});
test('unknown remote snapshot disables mutations but guest local records stay usable', async () => {
  const value = { ...page(), remoteFetchedAt: null, remoteIncluded: false };
  const model = useHistory(backend({ historyPage: async () => reply(value) }));
  model.connect(account);
  await model.open('song');
  assert.equal(model.canMutate.value, false);
  value.canSync = false;
  await model.localRefresh();
  assert.equal(model.canMutate.value, true);
});
test('a lost mutation reply is read back from the journal without repeating the write', async () => {
  let writes = 0;
  const value = page();
  const model = useHistory(
    backend({
      historyPage: async () => reply(value),
      historyCommand: async () => {
        writes++;
        value.operations = [
          {
            id: intent().id,
            action: 'clear',
            selectedCount: null,
            phase: 'uncertain',
            createdAt: 1,
            localRemoved: 2,
            remotePlanned: true,
            lastReadAt: null,
          },
        ];
        throw { code: 'network', message: '结果未知' };
      },
    }),
  );
  model.connect(account);
  await model.open('song');
  await model.command(intent());
  assert.equal(model.pending.value, true);
  assert.equal(model.uncertain.value, undefined);
  await model.command({ ...intent(), id: '00000000-0000-0000-0000-000000000002' });
  assert.equal(writes, 1);
  assert.equal(model.page.value?.operations[0]?.phase, 'uncertain');
});
test('unseen intents survive tab switches and cancellation reserves the same UUID', async () => {
  const calls: HistoryCommand[] = [];
  const model = useHistory(
    backend({
      historyCommand: async (e, k, c) => {
        calls.push(c);
        if (c.kind === 'mutate') throw { code: 'ipc', message: '连接丢失' };
        return reply(
          {
            ...page(k),
            operations: [
              {
                id: intent().id,
                action: 'clear',
                selectedCount: null,
                phase: 'cancelled',
                createdAt: 1,
                localRemoved: 0,
                remotePlanned: false,
                lastReadAt: null,
              },
            ],
          },
          e,
        );
      },
    }),
  );
  model.connect(account);
  await model.open('song');
  await model.command(intent());
  assert.equal(model.uncertain.value?.id, intent().id);
  await model.open('album');
  assert.equal(model.pending.value, false);
  await model.open('song');
  assert.equal(model.pending.value, true);
  await model.setSync(false);
  assert.equal(calls.length, 1);
  await model.cancelUnknown();
  assert.equal(model.pending.value, false);
  assert.deepEqual(calls, [intent(), { ...intent(), kind: 'cancel_unseen' }]);
});
test('late reads cannot leak across accounts, tabs or disposal', async () => {
  for (const mode of ['account', 'tab', 'dispose']) {
    const late = deferred<ReturnType<typeof reply>>();
    const model = useHistory(
      backend({ historyPage: async (e, k) => (k === 'song' ? late.promise : reply(page(k), e)) }),
    );
    model.connect(account);
    const first = model.open('song');
    if (mode === 'account') model.connect({ ...account, epoch: '2' });
    else if (mode === 'dispose') model.dispose();
    else await model.open('album');
    late.resolve(reply());
    await first;
    assert.equal(model.page.value?.kind, mode === 'tab' ? 'album' : undefined);
  }
});
test('mismatched native category and expired session are surfaced, not treated as empty history', async () => {
  let expired = 0;
  const model = useHistory(
    backend({ historyPage: async () => reply(page('album')) }),
    () => expired++,
  );
  model.connect(account);
  await model.open('song');
  assert.equal(model.page.value, undefined);
  assert.ok(model.error.value);
  const ended = useHistory(
    backend({
      historyPage: async () => {
        throw { code: 'session_expired', message: '登录失效' };
      },
    }),
    () => expired++,
  );
  ended.connect(account);
  await ended.open('song');
  assert.equal(ended.page.value, undefined);
  assert.equal(expired, 1);
});
test('history labels distinguish unknown time and partial rejection', () => {
  assert.equal(historyTime(0).short, '时间未知');
  const today = new Date(2026, 9, 28, 14, 20);
  assert.match(historyTime(today.getTime(), today).short, /今天/);
  assert.match(historyTime(new Date(2026, 9, 27, 12).getTime(), today).short, /昨天/);
  assert.match(
    historyOutcome({
      id: 'x',
      action: 'clear',
      selectedCount: null,
      phase: 'rejected',
      createdAt: 1,
      localRemoved: 3,
      remotePlanned: true,
      lastReadAt: null,
    }),
    /本机已移除 3/,
  );
});

test('local history remains reachable before login and after a guest 301', async () => {
  for (const status of ['unknown', 'expired', 'signed_out'] as const) {
    let reads = 0;
    const model = useHistory(
      backend({
        historyPage: async (e, k) => {
          reads++;
          return reply(
            { ...page(k), canSync: false, remoteIncluded: false, remoteFetchedAt: null },
            e,
          );
        },
      }),
    );
    model.connect({ ...account, status });
    await model.open('song');
    assert.ok(reads > 0);
    assert.equal(model.page.value?.canSync, false);
    assert.equal(model.canMutate.value, true);
  }
});
