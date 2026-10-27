import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useMvSocial } from '../src/composables/useMvSocial.ts';
import type { MvClient, MvDetail, MvLikeOperation, MvRecommendations } from '../src/services/mv.ts';
import type { AccountData } from '../src/services/contracts.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
const envelope = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
const recommendations = (id = '123'): MvRecommendations => ({ id, items: [], cached: false });
const detail = (id = '123', liked = false): MvDetail => ({
  card: {
    id,
    title: 'MV',
    artists: [],
    coverUrl: null,
    durationMs: 1000,
    playCount: 0,
    subscribed: false,
  },
  description: '',
  publishedDate: '',
  playResolutions: [],
  downloads: [],
  cached: false,
  liked,
  likeCount: 77,
  commentCount: 0,
  likeOperation: null,
  relatedSongs: [],
  relatedPrivilegeFailed: false,
});
const operation = (id = '123'): MvLikeOperation => ({
  id,
  desired: true,
  observed: true,
  count: 77,
  phase: 'confirmed',
});
const client = (overrides: Partial<MvClient> = {}): MvClient =>
  ({
    mvRecommendations: async (_e, id) => envelope(recommendations(id)),
    mvLike: async () => envelope(operation()),
    mvLikeReconcile: async () => envelope(operation()),
    mvDetail: async (_e, id) => envelope(detail(id)),
    ...overrides,
  }) as MvClient;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
const turn = () => new Promise((r) => setImmediate(r));
test('late MV detail cannot undo a confirmed resource like or invent global count increments', async () => {
  const model = useMvSocial(client(), () => {});
  model.connect(account());
  const ticket = model.activate('123');
  model.acceptDetail(detail(), ticket);
  await model.like(true);
  model.acceptDetail(detail(), ticket);
  assert.equal(model.liked.value, true);
  assert.equal(model.count.value, 77);
  assert.equal(model.pending.value, undefined);
});
test('unknown MV write blocks a repeat and exposes native read-only reconciliation', async () => {
  let writes = 0,
    reads = 0;
  const model = useMvSocial(
    client({
      mvLike: async () => {
        writes++;
        throw { code: 'network', message: 'lost' };
      },
      mvDetail: async () =>
        envelope({
          ...detail(),
          likeOperation: { ...operation(), phase: 'uncertain', observed: null, count: null },
        }),
      mvLikeReconcile: async () => {
        reads++;
        return envelope(operation());
      },
    }),
    () => {},
  );
  model.connect(account());
  model.acceptDetail(detail(), model.activate('123'));
  await model.like(true);
  assert.equal(model.error.value, 'lost');
  assert.equal(model.pending.value?.phase, 'uncertain');
  await model.like(true);
  assert.equal(writes, 1);
  await model.like(true, true);
  assert.equal(writes, 1);
  assert.equal(reads, 1);
  assert.equal(model.liked.value, true);
  assert.equal(model.pending.value, undefined);
});
test('old-account writes and old-resource recommendations cannot enter the new view', async () => {
  const write = deferred<AccountData<MvLikeOperation>>(),
    read = deferred<AccountData<MvRecommendations>>();
  const model = useMvSocial(
    client({ mvLike: () => write.promise, mvRecommendations: () => read.promise }),
    () => {},
  );
  model.connect(account());
  model.acceptDetail(detail(), model.activate('123'));
  const pending = model.like(true);
  model.connect(account('2'));
  model.activate('456');
  write.resolve(envelope(operation()));
  read.resolve(envelope(recommendations()));
  await pending;
  await turn();
  assert.equal(model.liked.value, null);
  assert.equal(model.pending.value, undefined);
  assert.equal(model.recommendations.value, undefined);
  assert.equal(model.busy.value, false);
});
test('failed recommendations retain explicitly marked cache without changing like state', async () => {
  const calls: boolean[] = [];
  const model = useMvSocial(
    client({
      mvRecommendations: async (_e, id, refresh) => {
        calls.push(refresh);
        if (refresh) throw { code: 'network', message: 'offline' };
        return envelope({ ...recommendations(id), cached: true });
      },
    }),
    () => {},
  );
  model.connect(account());
  model.acceptDetail(detail('123', true), model.activate('123'));
  await turn();
  assert.deepEqual(calls, [true, false]);
  assert.equal(model.recommendations.value?.cached, true);
  assert.equal(model.recommendationError.value, 'offline');
  assert.equal(model.liked.value, true);
});
