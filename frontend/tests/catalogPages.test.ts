import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useCatalogPages } from '../src/composables/useCatalogPages.ts';
import { useCatalogMembership } from '../src/composables/useCatalogMembership.ts';
import { nextCatalogQuery } from '../src/services/catalogPages.ts';
import type {
  CatalogQuery,
  CatalogView,
  CatalogMembershipView,
  CatalogMembershipAction,
} from '../src/services/catalog.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'fixture',
  avatarUrl: null,
};
const resource = { kind: 'artist' as const, id: '12' };
const query: CatalogQuery = { kind: 'artist_songs', id: '12', offset: 0, limit: 20 };
function view(q: CatalogQuery = query, cached = false): CatalogView {
  return {
    query: q,
    cached,
    fetchedAt: 1,
    revision: 'r',
    page: {
      kind: 'artist_songs',
      offset: 'offset' in q ? q.offset : 0,
      total: 55,
      more: 'offset' in q ? q.offset < 40 : false,
      tracks: [],
    },
  };
}
const defer = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
};
test('page aggregation keeps a visible snapshot during refresh and invalidates old continuations after success', async () => {
  let fail = false;
  const m = useCatalogPages({
    catalogPage: async (e, q, refresh) => {
      if (refresh && fail) throw { code: 'network', message: 'offline' };
      return { epoch: e, value: view(q, !refresh) };
    },
  });
  m.connect(account);
  await m.open(query);
  await m.more();
  assert.equal(m.pages.value.length, 2);
  fail = true;
  await m.open(query, true);
  assert.equal(m.pages.value.length, 2);
  assert.equal(m.error.value, 'offline');
  fail = false;
  await m.open(query, true);
  assert.equal(m.pages.value.length, 1);
  m.dispose();
});
test('first-page cache appears before network settles; account change drops that page and pending continuations', async () => {
  const late = defer<{ epoch: string; value: CatalogView }>();
  const m = useCatalogPages({
    catalogPage: async (e, q, refresh) =>
      refresh ? late.promise : { epoch: e, value: view(q, true) },
  });
  m.connect(account);
  const pending = m.open(query);
  await new Promise((r) => setImmediate(r));
  assert.equal(m.pages.value.length, 1);
  assert.equal(m.pages.value[0]?.cached, true);
  m.connect({ ...account, epoch: '2' });
  late.resolve({ epoch: '1', value: view() });
  await pending;
  assert.equal(m.pages.value.length, 0);
  m.dispose();
});
test('continuation failure preserves earlier pages and retry appends the same next page once', async () => {
  const calls: number[] = [];
  let fail = true;
  const m = useCatalogPages({
    catalogPage: async (e, q, refresh) => {
      if (refresh && 'offset' in q) {
        calls.push(q.offset);
        if (q.offset === 20 && fail) throw { code: 'network', message: 'offline' };
      }
      return { epoch: e, value: view(q, !refresh) };
    },
  });
  m.connect(account);
  await m.open(query);
  await m.more();
  assert.equal(m.pages.value.length, 1);
  fail = false;
  await m.more();
  assert.equal(m.pages.value.length, 2);
  assert.deepEqual(calls, [0, 20, 20]);
  m.dispose();
});
test('large video cursors retain decimal precision and stalled pagination stops', () => {
  const v: CatalogView = {
    ...view(),
    query: { kind: 'artist_videos', id: '12', cursor: '0', size: 20 },
    page: {
      kind: 'artist_videos',
      nextCursor: '90071992547409931234',
      more: true,
      size: 20,
      videos: [],
    },
  };
  assert.deepEqual(nextCatalogQuery(v), {
    kind: 'artist_videos',
    id: '12',
    cursor: '90071992547409931234',
    size: 20,
  });
  v.page.nextCursor = '000';
  assert.equal(nextCatalogQuery(v), null);
});
test('a lost membership reply inspects the journal without resending and requires reconciliation', async () => {
  const calls: string[] = [];
  let written = false;
  const m = useCatalogMembership({
    catalogMembership: async (epoch, resource, action) => {
      calls.push(action.kind);
      if (action.kind === 'set') {
        written = true;
        throw { code: 'network', message: 'reply lost' };
      }
      const value: CatalogMembershipView = {
        resource,
        operation: written
          ? {
              phase: action.kind === 'reconcile' ? 'confirmed' : 'uncertain',
              desired: true,
              observed: action.kind === 'reconcile' ? true : null,
            }
          : null,
      };
      return { epoch, value };
    },
  });
  m.connect(account, resource);
  await m.command({ kind: 'inspect' });
  await m.command({ kind: 'set', desired: true });
  assert.equal(m.pending.value, true);
  assert.equal(m.error.value, 'reply lost');
  await m.command({ kind: 'set', desired: true });
  assert.deepEqual(calls, ['inspect', 'set', 'inspect']);
  assert.equal(await m.command({ kind: 'reconcile' }), true);
  assert.equal(m.pending.value, false);
  m.dispose();
});
test('old journal success never overrides current dynamic state merely by inspecting it', async () => {
  const m = useCatalogMembership({
    catalogMembership: async (epoch, resource) => ({
      epoch,
      value: { resource, operation: { phase: 'confirmed', desired: true, observed: true } },
    }),
  });
  m.connect(account, resource);
  assert.equal(await m.command({ kind: 'inspect' }), null);
  m.dispose();
});
test('late collection response is dropped after changing accounts', async () => {
  const late = defer<{ epoch: string; value: CatalogMembershipView }>();
  let pending: CatalogMembershipAction | undefined;
  const m = useCatalogMembership({
    catalogMembership: async (epoch, resource, action) => {
      pending = action;
      return action.kind === 'inspect'
        ? { epoch, value: { resource, operation: null } }
        : late.promise;
    },
  });
  m.connect(account, resource);
  await m.command({ kind: 'inspect' });
  const write = m.command({ kind: 'set', desired: true });
  assert.equal(pending?.kind, 'set');
  m.connect({ ...account, epoch: '2' }, resource);
  late.resolve({
    epoch: '1',
    value: { resource, operation: { phase: 'confirmed', desired: true, observed: true } },
  });
  assert.equal(await write, null);
  assert.equal(m.operation.value, null);
  assert.equal(m.known.value, false);
  m.dispose();
});
