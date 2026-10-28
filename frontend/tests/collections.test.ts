import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useCollections } from '../src/composables/useCollections.ts';
import { normalizeKeyword } from '../src/services/keyword.ts';
import type { CollectionQuery, CollectionView } from '../src/services/collections.ts';
import vectors from './fixtures/keyword-vectors.json' with { type: 'json' };
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'fixture',
  avatarUrl: null,
};
function view(query: CollectionQuery, cached = false): CollectionView {
  return {
    query,
    cached,
    fetchedAt: 1,
    total: null,
    next:
      query.kind === 'saved_albums'
        ? { ...query, offset: query.offset + query.limit }
        : query.kind === 'followed_artists'
          ? { ...query, cursor: query.cursor === '0' ? '90071992547409931234' : '0' }
          : null,
    page:
      query.kind === 'followed_artists'
        ? { kind: 'follows', entries: [] }
        : { kind: 'albums', albums: [] },
  };
}
const defer = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
};
test('collection query normalization matches executed official M123 vectors', () => {
  for (const v of vectors) assert.equal(normalizeKeyword(v.input), v.expected);
  assert.equal(normalizeKeyword('Ａ [LIVE]'), normalizeKeyword('a [live]'));
});
test('collection first page paints cache immediately, always refreshes, and drops late replies on account switch', async () => {
  const late = defer<{ epoch: string; value: CollectionView }>();
  const calls: boolean[] = [];
  const m = useCollections({
    collectionPage: async (e, q, refresh) => {
      calls.push(refresh);
      return refresh ? late.promise : { epoch: e, value: view(q, true) };
    },
  });
  m.connect(account);
  const pending = m.open('saved_albums');
  await new Promise((r) => setImmediate(r));
  assert.equal(m.pages.value.length, 1);
  assert.equal(m.pages.value[0]?.cached, true);
  assert.deepEqual(calls, [false, true]);
  m.connect({ ...account, epoch: '2' });
  late.resolve({ epoch: '1', value: view({ kind: 'saved_albums', offset: 0, limit: 50 }) });
  await pending;
  assert.equal(m.pages.value.length, 0);
  assert.equal(m.loading.value, false);
  m.dispose();
});
test('failed refresh retains every loaded page, and retry repeats the first page rather than appending', async () => {
  let fail = false;
  const offsets: number[] = [];
  const m = useCollections({
    collectionPage: async (e, q, refresh) => {
      if (refresh && q.kind === 'saved_albums') {
        offsets.push(q.offset);
        if (fail) throw { code: 'network', message: 'offline' };
      }
      return { epoch: e, value: view(q, !refresh) };
    },
  });
  m.connect(account);
  await m.open('saved_albums');
  await m.more();
  fail = true;
  await m.refresh();
  assert.equal(m.pages.value.length, 2);
  assert.equal(m.error.value, 'offline');
  fail = false;
  await m.retry();
  assert.equal(m.pages.value.length, 1);
  assert.deepEqual(offsets, [0, 50, 0, 0]);
  m.dispose();
});
test('failed continuation is read again once and switching scope cancels its late reply', async () => {
  let fail = true;
  const late = defer<{ epoch: string; value: CollectionView }>();
  let wait = false;
  const m = useCollections({
    collectionPage: async (e, q, refresh) => {
      if (refresh && q.kind === 'saved_albums' && q.offset > 0) {
        if (fail) throw { code: 'network', message: 'offline' };
        if (wait) return late.promise;
      }
      return { epoch: e, value: view(q, !refresh) };
    },
  });
  m.connect(account);
  await m.open('saved_albums');
  await m.more();
  assert.equal(m.pages.value.length, 1);
  fail = false;
  await m.retry();
  assert.equal(m.pages.value.length, 2);
  wait = true;
  const pending = m.more();
  await m.open('purchased_albums');
  late.resolve({ epoch: '1', value: view({ kind: 'saved_albums', offset: 100, limit: 50 }) });
  await pending;
  assert.equal(m.pages.value.length, 1);
  assert.equal(m.pages.value[0]?.query.kind, 'purchased_albums');
  m.dispose();
});
test('opaque cursor precision is preserved and a cycle produces an explicit refresh recovery', async () => {
  const cursors: string[] = [];
  const m = useCollections({
    collectionPage: async (e, q, refresh) => {
      if (refresh && q.kind === 'followed_artists') cursors.push(q.cursor);
      return { epoch: e, value: view(q, !refresh) };
    },
  });
  m.connect(account);
  await m.open('followed_artists');
  await m.more();
  await m.more();
  assert.deepEqual(cursors, ['0', '90071992547409931234']);
  assert.match(m.error.value, /分页/);
  await m.retry();
  assert.deepEqual(cursors, ['0', '90071992547409931234', '0']);
  assert.equal(m.pages.value.length, 1);
  m.dispose();
});
test('wrong scope response does not appear and expired account clears its recovery page', async () => {
  let expired = 0,
    bad = false;
  const m = useCollections(
    {
      collectionPage: async (e, q, refresh) => {
        if (refresh && bad) throw { code: 'session_expired', message: 'expired' };
        return { epoch: e, value: view(q, !refresh) };
      },
    },
    () => expired++,
  );
  m.connect(account);
  await m.open('saved_albums');
  bad = true;
  await m.refresh();
  assert.equal(expired, 1);
  assert.equal(m.pages.value.length, 0);
  m.dispose();
  const n = useCollections({
    collectionPage: async (e) => ({ epoch: e, value: view({ kind: 'purchased_albums' }) }),
  });
  n.connect(account);
  await n.open('saved_albums');
  assert.equal(n.pages.value.length, 0);
  assert.match(n.error.value, /不匹配/);
  n.dispose();
});
