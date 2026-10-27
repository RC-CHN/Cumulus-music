import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useComments } from '../src/composables/useComments.ts';
import {
  commentQuery,
  type CommentClient,
  type CommentItem,
  type CommentOperation,
  type CommentPage,
  type CommentQuery,
  type CommentResource,
} from '../src/services/comments.ts';
import type { AccountData } from '../src/services/contracts.ts';
const resource: CommentResource = { kind: 'track', id: '123' };
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
const item = (liked = false): CommentItem => ({
  id: '101',
  content: 'hello',
  time: 1700000000000,
  timeText: '',
  liked,
  likedCount: 77,
  author: null,
  replies: [],
  owned: false,
});
const page = (value = item()): CommentPage => ({
  resource,
  comments: [value],
  hotComments: [value],
  topComments: [],
  currentComment: null,
  total: 40,
  more: true,
  moreHot: false,
  next: { ...commentQuery(), offset: 20, beforeTime: 1700000000000 },
  cached: false,
  operations: [],
});
const record = (): CommentOperation => ({
  id: '101',
  desired: true,
  phase: 'confirmed',
  observed: item(true),
});
const envelope = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { resolve, promise };
}
function client(overrides: Partial<CommentClient> = {}): CommentClient {
  return {
    commentsPage: async () => envelope(page()),
    commentsLike: async () => envelope(record()),
    commentsReconcile: async () => envelope(record()),
    ...overrides,
  };
}
test('old-account page and like replies cannot populate a new account', async () => {
  const wait = deferred<AccountData<CommentPage>>();
  const model = useComments(client({ commentsPage: () => wait.promise }));
  model.connect(account());
  const request = model.open(resource);
  model.connect(account('2'));
  wait.resolve(envelope(page()));
  await request;
  assert.equal(model.page.value, undefined);
  assert.equal(model.loading.value, false);
});
test('unknown write only exposes explicit readback and never repeats the mutation', async () => {
  let writes = 0,
    checks = 0;
  const model = useComments(
    client({
      commentsLike: async () => {
        writes++;
        throw { code: 'network', message: 'lost' };
      },
      commentsReconcile: async () => {
        checks++;
        return envelope(record());
      },
      commentsPage: async (_e, _r, _q, refresh) =>
        envelope({
          ...page(),
          cached: !refresh,
          operations: refresh
            ? []
            : [{ id: '101', desired: true, phase: 'uncertain', observed: item() }],
        }),
    }),
  );
  model.connect(account());
  await model.open(resource);
  await model.like(item());
  assert.equal(model.actionError.value, 'lost');
  assert.equal(model.page.value?.operations.length, 1);
  await model.like(item());
  assert.equal(writes, 1);
  await model.reconcile('101');
  assert.equal(writes, 1);
  assert.equal(checks, 1);
  assert.equal(model.page.value?.operations.length, 0);
  assert.equal(model.page.value?.comments[0]?.liked, true);
  assert.equal(model.page.value?.hotComments[0]?.liked, true);
});
test('a slow page cannot overwrite a later confirmed like or its global count', async () => {
  const late = deferred<AccountData<CommentPage>>();
  let requests = 0;
  const model = useComments(
    client({ commentsPage: async () => (++requests === 1 ? envelope(page()) : late.promise) }),
  );
  model.connect(account());
  await model.open(resource);
  const pending = model.load();
  await model.like(item());
  late.resolve(envelope(page()));
  await pending;
  assert.equal(model.page.value?.comments[0]?.liked, true);
  assert.equal(model.page.value?.comments[0]?.likedCount, 77);
});
test('paging uses native cursors and does not invent a hot-page flag from totals', async () => {
  const seen: CommentQuery[] = [];
  const model = useComments(
    client({
      commentsPage: async (_e, r, q) => {
        seen.push(q);
        return envelope({
          ...page(),
          resource: r,
          total: 100,
          more: false,
          next: q.offset === 0 ? { ...q, offset: 17, beforeTime: 1234 } : null,
        });
      },
    }),
  );
  model.connect(account());
  await model.open(resource);
  await model.next();
  assert.equal(model.query.value.offset, 17);
  assert.equal(seen[1]?.beforeTime, 1234);
  assert.equal(model.history.value.length, 1);
  assert.equal(model.page.value?.next, null);
  await model.back();
  assert.equal(model.query.value.offset, 0);
  await model.sort('hot');
  assert.equal(model.query.value.sort, 'hot');
  assert.equal(model.history.value.length, 0);
});
test('failed refresh retains explicit cache state and does not erase write errors', async () => {
  let fail = false;
  const model = useComments(
    client({
      commentsPage: async (_e, _r, _q, refresh) => {
        if (fail && refresh) throw { code: 'network', message: 'offline' };
        return envelope({ ...page(), cached: !refresh });
      },
    }),
  );
  model.connect(account());
  await model.open(resource);
  model.actionError.value = 'verify first';
  fail = true;
  await model.load();
  assert.equal(model.page.value?.cached, true);
  assert.equal(model.error.value, 'offline');
  assert.equal(model.actionError.value, 'verify first');
});
test('changing resource revokes pending results and locate keeps IDs as strings', async () => {
  const held = deferred<AccountData<CommentOperation>>();
  const seen: CommentQuery[] = [];
  const model = useComments(
    client({
      commentsLike: () => held.promise,
      commentsPage: async (_e, r, q) => {
        seen.push(q);
        return envelope({ ...page(), resource: r });
      },
    }),
  );
  model.connect(account());
  await model.open(resource);
  const pending = model.like(item());
  await model.open({ kind: 'mv', id: '456' });
  held.resolve(envelope(record()));
  await pending;
  assert.equal(model.page.value?.resource.kind, 'mv');
  assert.equal(model.page.value?.comments[0]?.liked, false);
  await model.locate('900719925474099312345');
  assert.equal(seen.at(-1)?.commentId, '900719925474099312345');
  assert.equal(model.busy.value.size, 0);
});
