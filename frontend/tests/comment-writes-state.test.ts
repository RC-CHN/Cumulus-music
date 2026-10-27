import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useCommentWrites } from '../src/composables/useCommentWrites.ts';
import { useCommentFloor } from '../src/composables/useCommentFloor.ts';
import type { CommentWrite, CommentWriteClient } from '../src/services/commentWrites.ts';
import type { CommentItem, CommentResource } from '../src/services/comments.ts';
import type {
  CommentFloorPage,
  CommentFloorClient,
  CommentFloorQuery,
} from '../src/services/commentFloor.ts';
import type { AccountData } from '../src/services/contracts.ts';
const resource: CommentResource = { kind: 'track', id: '10' };
const item = (id = '101'): CommentItem => ({
  id,
  parentId: null,
  content: 'original',
  time: 1,
  timeText: '',
  liked: false,
  likedCount: 0,
  author: { name: 'Example', avatarUrl: null },
  owned: true,
  replies: [],
});
const data = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
const row = (phase: CommentWrite['phase'] = 'prepared'): CommentWrite => ({
  id: 'intent',
  resource,
  command: { kind: 'publish', content: '  draft 🎵 ', replyTo: null },
  phase,
  createdAt: 1,
  acknowledged: false,
  receiptId: null,
  target: null,
  observed: null,
  observedAbsent: false,
  serviceCode: null,
});
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
const turn = () => new Promise((r) => setImmediate(r));
function core(overrides: Partial<CommentWriteClient> = {}): CommentWriteClient {
  return {
    commentsWriteOperations: async () => data([]),
    commentsPrepareWrite: async () => data(row()),
    commentsWriteCommand: async () => data(row('confirmed')),
    ...overrides,
  };
}
test('comment text intent is prepared once, double submit is blocked and exact submitted draft is cleared', async () => {
  let prepared = 0,
    sent = 0,
    saved = 0;
  const gate = deferred<AccountData<CommentWrite>>();
  const model = useCommentWrites(
    core({
      commentsPrepareWrite: async (_e, _r, c) => {
        prepared++;
        assert.deepEqual(c, row().command);
        return data(row());
      },
      commentsWriteCommand: () => {
        sent++;
        return gate.promise;
      },
    }),
    async () => {
      saved++;
    },
  );
  await model.connect('1', resource);
  model.draft.value = '  draft 🎵 ';
  const publishing = model.publish();
  await turn();
  await model.publish();
  assert.equal(prepared, 1);
  assert.equal(sent, 1);
  gate.resolve(data(row('confirmed')));
  await publishing;
  assert.equal(model.draft.value, '');
  assert.equal(saved, 1);
  assert.equal(model.pending.value.length, 0);
  model.disconnect();
});
test('switching resource or account during prepare retains responsibility but never sends the old prepared intent', async () => {
  const gate = deferred<AccountData<CommentWrite>>();
  let sent = 0;
  const model = useCommentWrites(
    core({
      commentsPrepareWrite: () => gate.promise,
      commentsWriteCommand: async () => {
        sent++;
        return data(row('confirmed'));
      },
    }),
  );
  await model.connect('1', resource);
  model.draft.value = 'draft';
  const writing = model.publish();
  await model.connect('2', { kind: 'track', id: '20' });
  gate.resolve(data(row()));
  await writing;
  assert.equal(sent, 0);
  assert.equal(model.operations.value.length, 0);
  assert.equal(model.draft.value, '');
  assert.equal(model.busy.value, false);
  model.disconnect();
});
test('lost public receipt restores pending record without preparing or sending again; reconciliation is explicit', async () => {
  let current: CommentWrite | undefined,
    prepares = 0;
  const actions: string[] = [];
  const model = useCommentWrites(
    core({
      commentsPrepareWrite: async () => {
        prepares++;
        current = row();
        return data(current);
      },
      commentsWriteOperations: async () => data(current ? [current] : []),
      commentsWriteCommand: async (_e, _id, c) => {
        actions.push(c.kind);
        if (c.kind === 'submit') {
          current = row('uncertain');
          throw { code: 'network', message: 'lost reply' };
        }
        current = row('confirmed');
        return data(current);
      },
    }),
  );
  await model.connect('1', resource);
  model.draft.value = '  draft 🎵 ';
  await model.publish();
  assert.equal(model.pending.value.length, 1);
  assert.equal(model.draft.value, '  draft 🎵 ');
  assert.ok(model.error.value.includes('不要重复提交'));
  await model.publish();
  assert.equal(prepares, 1);
  assert.deepEqual(actions, ['submit']);
  model.draft.value = 'new local words';
  await model.action(model.pending.value[0]!, { kind: 'reconcile' });
  assert.equal(model.draft.value, 'new local words');
  assert.deepEqual(actions, ['submit', 'reconcile']);
  model.disconnect();
});
test('old operations refresh and late successful write cannot overwrite a newer account', async () => {
  const oldRead = deferred<AccountData<CommentWrite[]>>(),
    oldWrite = deferred<AccountData<CommentWrite>>();
  let reads = 0,
    saved = 0;
  const model = useCommentWrites(
    core({
      commentsWriteOperations: async (e) => (++reads === 2 ? oldRead.promise : data([], e)),
      commentsWriteCommand: () => oldWrite.promise,
    }),
    async () => {
      saved++;
    },
  );
  await model.connect('1', resource);
  const reading = model.refresh();
  model.draft.value = '  draft 🎵 ';
  const writing = model.publish();
  await turn();
  oldRead.resolve(data([row('uncertain')]));
  await reading;
  assert.equal(model.operations.value[0]?.phase, 'prepared');
  await model.connect('2', resource);
  oldWrite.resolve(data(row('confirmed')));
  await writing;
  assert.equal(model.operations.value.length, 0);
  assert.equal(saved, 0);
  model.disconnect();
});
test('expiry clears account drafts and stops follow-up reads and writes', async () => {
  let reads = 0,
    writes = 0,
    expired = 0;
  const model = useCommentWrites(
    core({
      commentsWriteOperations: async () => {
        reads++;
        return data([]);
      },
      commentsWriteCommand: async () => {
        writes++;
        throw { code: 'session_expired', message: 'expired' };
      },
    }),
    async () => {},
    () => {},
    () => {
      expired++;
    },
  );
  await model.connect('1', resource);
  model.draft.value = 'draft';
  await model.publish();
  assert.equal(expired, 1);
  assert.equal(reads, 1);
  assert.equal(writes, 1);
  assert.equal(model.draft.value, '');
  assert.equal(model.operations.value.length, 0);
  model.draft.value = 'another';
  await model.publish();
  await model.refresh();
  assert.equal(writes, 1);
  assert.equal(reads, 1);
});
test('taking back a rejected reply never silently turns a missing target into a top-level comment', async () => {
  const model = useCommentWrites(core());
  await model.connect('1', resource);
  const rejected = {
    ...row('rejected'),
    command: { kind: 'publish' as const, content: 'reply', replyTo: '101' },
  };
  model.reuse(rejected);
  assert.equal(model.draft.value, '');
  assert.ok(model.error.value.includes('定位原评论'));
  model.reuse({ ...rejected, target: item() });
  assert.equal(model.draft.value, 'reply');
  assert.equal(model.reply.value?.id, '101');
  model.disconnect();
});
const floorPage = (query: CommentFloorQuery): CommentFloorPage => ({
  resource,
  query,
  owner: item(query.parentId),
  currentComment: null,
  comments: [{ ...item('201'), parentId: query.parentId }],
  bestComments: [],
  total: 2,
  more: true,
  next: { ...query, time: 2 },
  cached: false,
});
test('floor requests preserve server time, previous pages and old roots cannot replace a newer selection', async () => {
  const late = deferred<AccountData<CommentFloorPage>>();
  let hold = false;
  const calls: CommentFloorQuery[] = [];
  const client: CommentFloorClient = {
    commentsFloor: async (_e, _r, q) => {
      calls.push(q);
      if (hold) {
        hold = false;
        return late.promise;
      }
      return data(floorPage(q));
    },
  };
  const model = useCommentFloor(client);
  model.connect('1', resource);
  await model.open(item());
  await model.next();
  assert.equal(model.query.value?.time, 2);
  assert.equal(model.history.value.length, 1);
  await model.back();
  assert.equal(model.query.value?.time, -1);
  hold = true;
  const loading = model.open(item('101'));
  await model.open(item('102'));
  late.resolve(data(floorPage({ parentId: '101', time: -1, limit: 20 })));
  await loading;
  assert.equal(model.page.value?.query.parentId, '102');
  assert.equal(calls[1]?.time, 2);
  model.close();
});
test('floor cache fallback is scoped and expiry never asks for the old cache', async () => {
  let reads = 0,
    cached = 0,
    expired = 0;
  let fail = false;
  const client: CommentFloorClient = {
    commentsFloor: async (e, _r, q, refresh) => {
      reads++;
      if (refresh) {
        if (fail) throw { code: 'session_expired', message: 'expired' };
        throw { code: 'network', message: 'offline' };
      }
      cached++;
      return data({ ...floorPage(q), cached: true }, e);
    },
  };
  const model = useCommentFloor(client, () => {
    expired++;
  });
  model.connect('1', resource);
  await model.open(item());
  assert.equal(model.page.value?.cached, true);
  assert.equal(cached, 1);
  fail = true;
  await model.load();
  assert.equal(expired, 1);
  assert.equal(cached, 1);
  assert.equal(reads, 3);
  assert.equal(model.page.value, undefined);
  assert.equal(model.query.value, undefined);
});
