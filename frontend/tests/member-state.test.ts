import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useMember } from '../src/composables/useMember.ts';
import type { MemberClient, MemberView } from '../src/services/member.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'fixture',
  avatarUrl: null,
};
const page = (fresh = true): MemberView => ({
  revision: 'native',
  cached: true,
  fresh,
  fetchedAt: 1,
  warning: null,
  page: { kind: 'home', modules: [], messages: [] },
});
const rights = { serverNow: 1, refreshAfterMs: 10000, entitlements: [] };
const reply = (v = page(), epoch = '1') => ({ epoch, value: v });
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function client(changes: Partial<MemberClient> = {}): MemberClient {
  return {
    memberPage: async () => reply(),
    memberRights: async () => ({ epoch: '1', value: rights }),
    openMemberLink: async () => {},
    ...changes,
  };
}
test('member cache remains visible after failed refresh and explicit retry replaces it', async () => {
  let fail = true;
  const calls: boolean[] = [];
  const m = useMember(
    client({
      memberPage: async (_e, _q, refresh) => {
        calls.push(refresh);
        if (refresh && fail) throw { code: 'network', message: '网络中断' };
        return reply(page(refresh));
      },
    }),
  );
  m.connect(account);
  await m.open({ kind: 'home' });
  assert.deepEqual(calls, [false, true]);
  await m.refresh();
  assert.equal(m.error.value, '网络中断');
  assert.equal(m.view.value?.revision, 'native');
  fail = false;
  await m.refresh();
  assert.equal(m.error.value, '');
  assert.equal(m.view.value?.fresh, true);
  m.dispose();
});
test('overlapping dates and old account responses never overwrite the displayed date', async () => {
  const late = deferred<ReturnType<typeof reply>>();
  let count = 0;
  const m = useMember(
    client({
      memberPage: async () =>
        ++count === 1
          ? late.promise
          : reply({
              ...page(),
              page: { kind: 'history', dates: ['2026-10-26'], date: '2026-10-26', tracks: [] },
            }),
    }),
  );
  m.connect(account);
  const first = m.open({ kind: 'home' });
  await m.open({ kind: 'history', date: '2026-10-26' });
  late.resolve(reply());
  await first;
  assert.equal(m.view.value?.page.kind, 'history');
  const old = deferred<ReturnType<typeof reply>>();
  const n = useMember(client({ memberPage: () => old.promise }));
  n.connect(account);
  const pending = n.open({ kind: 'home' });
  n.connect({ ...account, epoch: '2' });
  old.resolve(reply());
  await pending;
  assert.equal(n.view.value, undefined);
  assert.equal(n.loading.value, false);
  m.dispose();
  n.dispose();
});
test('rights failure is independent from content and expired sessions stop followup requests', async () => {
  let expired = 0,
    requests = 0;
  const m = useMember(
    client({
      memberRights: async () => {
        requests++;
        throw { code: 'network', message: '权益读取失败' };
      },
    }),
    () => expired++,
  );
  m.connect(account);
  await m.open({ kind: 'home' });
  await m.loadRights(true);
  assert.equal(m.view.value?.page.kind, 'home');
  assert.equal(m.error.value, '');
  assert.equal(m.rightsError.value, '权益读取失败');
  assert.equal(expired, 0);
  m.dispose();
  const e = useMember(
    client({
      memberRights: async () => {
        requests++;
        throw { code: 'session_expired', message: '已过期' };
      },
    }),
    () => expired++,
  );
  e.connect(account);
  await e.loadRights();
  const before = requests;
  await e.loadRights(true);
  assert.equal(requests, before);
  assert.equal(expired, 1);
  assert.equal(e.rightsLoading.value, false);
  e.dispose();
});
test('external links use the displayed native revision and suppress late errors across accounts', async () => {
  const late = deferred<void>();
  const calls: unknown[] = [];
  const m = useMember(
    client({
      openMemberLink: async (...args) => {
        calls.push(args);
        await late.promise;
        throw { code: 'network', message: '旧窗口失败' };
      },
    }),
  );
  m.connect(account);
  await m.open({ kind: 'home' });
  const first = m.openExternal('0:1:0');
  await m.openExternal('0:1:0');
  assert.equal(calls.length, 1);
  assert.deepEqual(calls[0], ['1', 'native', '0:1:0']);
  m.connect({ ...account, epoch: '2' });
  late.resolve();
  await first;
  assert.equal(m.error.value, '');
  assert.equal(m.linkBusy.value, false);
  m.dispose();
});
test('same page in flight is not invalidated and no hidden expiry refresh is scheduled', async () => {
  const late = deferred<ReturnType<typeof reply>>();
  let reads = 0;
  const m = useMember(
    client({
      memberPage: async () => {
        reads++;
        return late.promise;
      },
    }),
  );
  m.connect(account);
  const a = m.open({ kind: 'home' });
  await m.open({ kind: 'home' });
  late.resolve(reply());
  await a;
  assert.equal(reads, 1);
  assert.equal(m.view.value?.revision, 'native');
  await m.loadRights();
  m.show(false);
  m.dispose();
});
