import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useComments } from '../src/composables/useComments.ts';
import { useCommentFloor } from '../src/composables/useCommentFloor.ts';
import {
  type CommentClient,
  type CommentItem,
  type CommentPage,
  type CommentQuery,
} from '../src/services/comments.ts';
import type {
  CommentFloorClient,
  CommentFloorPage,
  CommentFloorQuery,
} from '../src/services/commentFloor.ts';
const resource = { kind: 'track' as const, id: '1' };
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
const item: CommentItem = {
  id: '101',
  parentId: null,
  content: 'synthetic',
  time: 1,
  timeText: '',
  liked: false,
  likedCount: 0,
  author: null,
  replies: [],
  owned: false,
};
const data = <T>(value: T, epoch = '1') => ({ epoch, value });
function page(q: CommentQuery, cached = false): CommentPage {
  return {
    resource,
    comments: [{ ...item, id: String((q.sort === 'hot' ? 301 : 101) + q.offset) }],
    hotComments: [],
    topComments: [],
    currentComment: null,
    total: 45,
    more: true,
    moreHot: false,
    next: { ...q, offset: q.offset + 20, beforeTime: 1234, commentId: null },
    cached,
    operations: [],
  };
}
function client(read: CommentClient['commentsPage']): CommentClient {
  return {
    commentsPage: read,
    commentsLike: async () => {
      throw Error('read test must not mutate');
    },
    commentsReconcile: async () => {
      throw Error('read test must not reconcile writes');
    },
  };
}
test('failed next retains its exact cursor and retries that navigation only once', async () => {
  let fail = true;
  const seen: CommentQuery[] = [];
  const m = useComments(
    client(async (_e, _r, q, refresh) => {
      if (!refresh) throw Error('not cached');
      seen.push({ ...q });
      if (fail && q.offset > 0) throw { code: 'network', message: 'offline' };
      return data(page(q));
    }),
  );
  m.connect(account());
  await m.open(resource);
  const next = { ...m.page.value!.next! };
  await m.next();
  assert.equal(m.query.value.offset, 0);
  assert.equal(m.history.value.length, 0);
  assert.deepEqual(m.retryTarget.value, { query: next, navigation: 'next' });
  fail = false;
  await m.retry();
  assert.deepEqual(seen.slice(-2), [next, next]);
  assert.equal(m.query.value.offset, 20);
  assert.equal(m.history.value.length, 1);
  assert.equal(m.retryTarget.value, undefined);
  const count = seen.length;
  await m.retry();
  assert.equal(seen.length, count);
});
test('cached navigation may display but successful retry never duplicates its history entry', async () => {
  let fail = true;
  const m = useComments(
    client(async (_e, _r, q, refresh) => {
      if (fail && refresh && q.offset > 0) throw { code: 'network', message: 'offline' };
      return data(page(q, !refresh));
    }),
  );
  m.connect(account());
  await m.open(resource);
  await m.next();
  assert.equal(m.page.value?.cached, true);
  assert.equal(m.history.value.length, 1);
  assert.equal(m.retryTarget.value?.navigation, 'refresh');
  fail = false;
  await m.retry();
  assert.equal(m.page.value?.cached, false);
  assert.equal(m.history.value.length, 1);
  await m.back();
  assert.equal(m.query.value.offset, 0);
  assert.equal(m.history.value.length, 0);
});
test('cached back from the third page keeps the remaining older history when refreshed', async () => {
  let fail = false;
  const m = useComments(
    client(async (_e, _r, q, refresh) => {
      if (fail && refresh && q.offset === 20) throw { code: 'network', message: 'offline' };
      return data(page(q, !refresh));
    }),
  );
  m.connect(account());
  await m.open(resource);
  await m.next();
  await m.next();
  assert.equal(m.query.value.offset, 40);
  assert.equal(m.history.value.length, 2);
  fail = true;
  await m.back();
  assert.equal(m.query.value.offset, 20);
  assert.equal(m.history.value.length, 1);
  fail = false;
  await m.retry();
  assert.equal(m.query.value.offset, 20);
  assert.equal(m.history.value.length, 1);
  await m.back();
  assert.equal(m.query.value.offset, 0);
  assert.equal(m.history.value.length, 0);
});
test('failed sort and locate keep their targets, while a new selection or account clears them', async () => {
  let fail = true;
  const seen: CommentQuery[] = [];
  const m = useComments(
    client(async (_e, _r, q, refresh) => {
      seen.push({ ...q });
      if (!refresh || (fail && (q.sort === 'hot' || q.commentId)))
        throw { code: 'network', message: 'offline' };
      return data(page(q));
    }),
  );
  m.connect(account());
  await m.open(resource);
  await m.sort('hot');
  assert.equal(m.query.value.sort, 'latest');
  fail = false;
  await m.retry();
  assert.equal(m.query.value.sort, 'hot');
  fail = true;
  await m.locate('900719925474099312345');
  assert.equal(m.retryTarget.value?.query.commentId, '900719925474099312345');
  fail = false;
  await m.sort('latest');
  assert.equal(m.retryTarget.value, undefined);
  fail = true;
  await m.sort('hot');
  m.connect(account('2'));
  assert.equal(m.retryTarget.value, undefined);
  const count = seen.length;
  await m.retry();
  assert.equal(seen.length, count);
});
function floor(q: CommentFloorQuery, cached = false): CommentFloorPage {
  return {
    resource,
    query: { ...q },
    owner: item,
    currentComment: null,
    comments: [{ ...item, id: q.time === -1 ? '501' : '521', parentId: q.parentId }],
    bestComments: [],
    total: 40,
    more: true,
    next: { ...q, time: 1000 },
    cached,
  };
}
test('floor retries failed server cursors and does not pop or push history twice after cache fallback', async () => {
  let fail = true,
    cache = false;
  const seen: CommentFloorQuery[] = [];
  const core: CommentFloorClient = {
    commentsFloor: async (_e, _r, q, refresh) => {
      if (!refresh && !cache) throw Error('not cached');
      if (refresh) seen.push({ ...q });
      if (refresh && fail && q.time > 0) throw { code: 'network', message: 'offline' };
      return data(floor(q, !refresh));
    },
  };
  const m = useCommentFloor(core);
  m.connect('1', resource);
  await m.open(item);
  await m.next();
  assert.equal(m.query.value?.time, -1);
  assert.equal(m.history.value.length, 0);
  fail = false;
  await m.retry();
  assert.equal(m.query.value?.time, 1000);
  assert.equal(m.history.value.length, 1);
  assert.deepEqual(seen.slice(-2), [
    { parentId: '101', time: 1000, limit: 20 },
    { parentId: '101', time: 1000, limit: 20 },
  ]);
  await m.back();
  fail = true;
  cache = true;
  await m.next();
  assert.equal(m.page.value?.cached, true);
  fail = false;
  await m.retry();
  assert.equal(m.history.value.length, 1);
  await m.back();
  assert.equal(m.history.value.length, 0);
});
test('wrong floor query identities are not accepted, and closing removes a failed read intent', async () => {
  const m = useCommentFloor({
    commentsFloor: async (_e, _r, q, refresh) => {
      if (!refresh) throw Error('not cached');
      return data({ ...floor(q), query: { ...q, limit: 99 } });
    },
  });
  m.connect('1', resource);
  await m.open(item);
  assert.equal(m.page.value, undefined);
  assert.ok(m.error.value);
  assert.equal(m.retryTarget.value?.query.limit, 20);
  m.close();
  await m.retry();
  assert.equal(m.query.value, undefined);
  assert.equal(m.retryTarget.value, undefined);
});
