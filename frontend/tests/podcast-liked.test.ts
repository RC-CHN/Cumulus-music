import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ref } from 'vue';
import { createPodcastLikedBridge } from '../src/adapters/podcastLiked.ts';
import { usePodcastLiked } from '../src/composables/usePodcastLiked.ts';
import type { AccountData, AccountView } from '../src/services/contracts.ts';
import type {
  EpisodeView,
  PodcastLikedClient,
  PodcastLikedPage,
  PodcastLikedPageTarget,
  PodcastLikedQuery,
} from '../src/services/podcast.ts';

const epoch = '1';
const account = (id = epoch): AccountView => ({
  epoch: id,
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
});
const row = (id: string): EpisodeView => ({
  id,
  name: `episode ${id}`,
  coverUrl: null,
  creator: '',
  radioId: null,
  radioName: '',
  durationMs: 0,
  feeType: 0,
  liked: true,
  unavailable: false,
});
const view = (
  patch: Partial<PodcastLikedPage> = {},
  owner = epoch,
): AccountData<PodcastLikedPage> => ({
  epoch: owner,
  value: {
    visit: 'visit-1',
    revision: 'revision-1',
    episodes: [],
    loadedCount: 0,
    continuation: 'cursor-0',
    capped: false,
    count: null,
    ...patch,
  },
});
const next = (continuation: string, revision = 'revision-1'): PodcastLikedQuery => ({
  kind: 'next',
  visit: 'visit-1',
  revision,
  continuation,
});
const changed = { code: 'snapshot_changed', message: 'snapshot changed', retryable: true };
const deferred = <T>() => {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
};
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));

function setup(
  replies: ((epoch: string, query: PodcastLikedQuery) => Promise<AccountData<PodcastLikedPage>>)[],
  extra: Partial<PodcastLikedClient> = {},
) {
  const current = ref(account());
  const calls: { epoch: string; query: PodcastLikedQuery }[] = [];
  let expired = 0;
  const model = usePodcastLiked(
    () => current.value,
    () => expired++,
    {
      podcastLikedPage: (epoch, query) => {
        calls.push({ epoch, query });
        const reply = replies.shift();
        assert.ok(reply, `unexpected ${query.kind}`);
        return reply(epoch, query);
      },
      podcastLikedCount: () => assert.fail('unexpected count'),
      podcastLikedPageProgram: () => assert.fail('unexpected programme'),
      ...extra,
    },
  );
  return { model, current, calls, expired: () => expired };
}

test('reads one page at a time, preserves stable row actions and restores the prefix and scroll on Back', async () => {
  const a = row('81');
  const b = row('82');
  const restoredA = row('81');
  const fixture = setup([
    async () => view(),
    async () => view({ episodes: [a], loadedCount: 1, count: 2, continuation: 'cursor-1' }),
    async () => view({ episodes: [b], loadedCount: 2, continuation: null }),
    async () => view({ loadedCount: 2, continuation: null }),
    async () =>
      view({
        revision: 'revision-2',
        episodes: [restoredA, b],
        loadedCount: 2,
        continuation: null,
        count: 2,
      }),
  ]);
  const { model, current, calls } = fixture;
  await model.open();
  assert.deepEqual(
    calls.map(({ query }) => query),
    [{ kind: 'open' }, next('cursor-0')],
  );
  assert.equal(model.count.value, 2);
  assert.deepEqual(model.page.value?.episodes, [a]);
  assert.equal(model.captureProgram(row('81')), undefined, 'cannot forge a displayed row');
  const captured = model.captureProgram(a);
  assert.equal(captured?.live(), true);
  current.value = { ...current.value, nickname: 'Updated profile' };
  assert.equal(captured?.live(), true, 'a same-epoch profile refresh does not reset the visit');
  await model.next();
  assert.equal(captured?.live(), true, 'an append does not invalidate the observed row');
  assert.deepEqual(model.page.value?.episodes, [a, b]);
  assert.deepEqual(calls.at(-1)?.query, next('cursor-1'));
  await model.next();
  assert.equal(calls.length, 3, 'no continuation at terminal');
  await model.suspend(841);
  assert.equal(captured?.live(), false);
  assert.equal(model.page.value, undefined);
  await model.restore();
  assert.deepEqual(
    calls.slice(3).map(({ query }) => query),
    [
      { kind: 'suspend', visit: 'visit-1', revision: 'revision-1' },
      { kind: 'restore', visit: 'visit-1', revision: 'revision-1' },
    ],
  );
  assert.deepEqual(model.page.value?.episodes, [restoredA, b]);
  assert.equal(model.scrollTop.value, 841);
  assert.equal(model.captureProgram(restoredA)?.target.revision, 'revision-2');
  assert.equal(model.captureProgram(a), undefined);
});

test('serializes local route controls without waiting for obsolete page or row requests', async () => {
  const opening = deferred<AccountData<PodcastLikedPage>>();
  const held = deferred<AccountData<PodcastLikedPage>>();
  const program = deferred<AccountData<EpisodeView>>();
  const a = row('81');
  const { model, calls } = setup(
    [
      () => opening.promise,
      async () => view(),
      async () => view({ revision: 'revision-2' }),
      async () =>
        view({ revision: 'revision-2', episodes: [a], loadedCount: 1, continuation: 'cursor-1' }),
      () => held.promise,
      async () => view({ revision: 'revision-2', loadedCount: 1, continuation: null }),
      async () =>
        view({
          revision: 'revision-3',
          episodes: [row('81')],
          loadedCount: 1,
          continuation: 'cursor-1',
        }),
    ],
    {
      podcastLikedPageProgram: async (owner, target) => {
        assert.equal(owner, epoch);
        assert.deepEqual(target, { visit: 'visit-1', revision: 'revision-2', voiceId: '81' });
        return program.promise;
      },
    },
  );
  const open = model.open();
  await tick();
  const left = model.suspend(30);
  const back = model.restore();
  assert.deepEqual(
    calls.map(({ query }) => query.kind),
    ['open'],
  );
  opening.resolve(view());
  await Promise.all([open, left, back]);
  assert.deepEqual(
    calls.map(({ query }) => query.kind),
    ['open', 'suspend', 'restore'],
  );
  assert.equal(model.scrollTop.value, 30);
  // The original mount left before it could issue the first wire read.
  await model.next();
  const selection = model.resolveProgram(a);
  const continuation = model.next();
  await model.next();
  assert.equal(calls.length, 5, 'duplicate in-flight continuation is not invoked');
  const restore = Promise.all([model.suspend(100), model.restore()]);
  await restore;
  assert.equal(model.page.value?.revision, 'revision-3');
  held.resolve(
    view({ revision: 'revision-2', episodes: [row('82')], loadedCount: 2, continuation: null }),
  );
  program.resolve({ epoch, value: a });
  await continuation;
  assert.equal(await selection, undefined);
  assert.deepEqual(
    model.page.value?.episodes.map((episode) => episode.id),
    ['81'],
  );
  assert.equal(model.pending.value, false);
});

test('retry reuses exactly the failing continuation and an expired Back opens a fresh visit only', async () => {
  const a = row('81');
  const error = { code: 'network', message: 'temporary failure', retryable: true };
  const { model, calls } = setup([
    async () => view(),
    async () => view({ episodes: [a], loadedCount: 1, continuation: 'cursor-1' }),
    async () => {
      throw error;
    },
    async () => view({ episodes: [row('82')], loadedCount: 2, continuation: null }),
    async () => view({ loadedCount: 2, continuation: null }),
    async () => {
      throw changed;
    },
    async () => view({ visit: 'visit-2', revision: 'new', continuation: 'fresh' }),
    async () =>
      view({
        visit: 'visit-2',
        revision: 'new',
        episodes: [row('83')],
        loadedCount: 1,
        continuation: null,
      }),
  ]);
  await model.open();
  await model.next();
  assert.equal(model.issue.value, error.message);
  assert.deepEqual(model.page.value?.episodes, [a]);
  await model.next();
  assert.equal(calls.length, 3, 'the automatic tail observer must not spin on a failure');
  await model.retry();
  assert.deepEqual(
    calls.slice(2, 4).map(({ query }) => query),
    [next('cursor-1'), next('cursor-1')],
  );
  assert.deepEqual(
    model.page.value?.episodes.map((episode) => episode.id),
    ['81', '82'],
  );
  await model.suspend(80);
  await model.restore();
  assert.deepEqual(
    calls.slice(4).map(({ query }) => query.kind),
    ['suspend', 'restore', 'open', 'next'],
  );
  assert.deepEqual(
    model.page.value?.episodes.map((episode) => episode.id),
    ['83'],
  );
  assert.equal(model.scrollTop.value, 0);
});

test('discards wrong-epoch and old account callbacks without touching complete membership', async () => {
  const opening = deferred<AccountData<PodcastLikedPage>>();
  const oldCount = deferred<AccountData<number>>();
  const latestCount = deferred<AccountData<number>>();
  const { model, current, calls, expired } = setup(
    [
      () => opening.promise,
      async () => view({ visit: 'visit-2', revision: 'new', continuation: 'fresh' }, '2'),
      async () =>
        view(
          {
            visit: 'visit-2',
            revision: 'new',
            episodes: [row('83')],
            loadedCount: 1,
            continuation: null,
            count: 1,
          },
          '2',
        ),
      async () =>
        view({ visit: 'visit-2', revision: 'new', loadedCount: 1, continuation: null }, '2'),
      async () => view({ visit: 'visit-2', revision: 'renewed' }, '1'),
    ],
    { podcastLikedCount: (owner) => (owner === epoch ? oldCount.promise : latestCount.promise) },
  );
  const oldOpen = model.open();
  await tick();
  const pendingCount = model.readCount();
  current.value = account('2');
  const newOpen = model.open();
  const count = model.readCount();
  await newOpen;
  latestCount.resolve({ epoch: '2', value: 99 });
  await count;
  assert.equal(model.count.value, 1, 'a later accepted page supersedes the pending count');
  oldCount.resolve({ epoch, value: 600 });
  opening.resolve(view());
  await Promise.all([oldOpen, pendingCount]);
  assert.equal(model.count.value, 1);
  assert.deepEqual(
    model.page.value?.episodes.map((episode) => episode.id),
    ['83'],
  );
  assert.deepEqual(
    calls.map(({ epoch, query }) => [epoch, query.kind]),
    [
      ['1', 'open'],
      ['2', 'open'],
      ['2', 'next'],
    ],
  );
  await model.suspend();
  await model.restore();
  assert.equal(model.page.value, undefined);
  assert.ok(model.issue.value);
  assert.equal(calls.length, 5, 'a wrong-epoch Restore does not trigger another request');
  assert.equal(expired(), 0);
});

test('the independent count can update a hidden tab without opening or reading the liked list', async () => {
  const values = [2, 0];
  const { model, calls } = setup([], {
    podcastLikedCount: async (epoch) => ({ epoch, value: values.shift()! }),
  });
  await model.readCount();
  assert.equal(model.count.value, 2);
  await model.readCount();
  assert.equal(model.count.value, 0);
  assert.equal(model.page.value, undefined);
  assert.deepEqual(calls, []);
});

test('the adapter transports only the typed liked visit and row request shape', async () => {
  const calls: unknown[] = [];
  const client = createPodcastLikedBridge(
    async <T>(command: string, args: Record<string, unknown>) => {
      calls.push([command, args]);
      return { epoch } as T;
    },
  );
  const target: PodcastLikedPageTarget = { visit: 'v', revision: 'r', voiceId: '81' };
  const query: PodcastLikedQuery = { kind: 'next', visit: 'v', revision: 'r', continuation: 'c' };
  const widerQuery = { ...query, likeTime: 1100, uid: 'other', offset: 100 };
  const widerTarget = { ...target, radioId: '80', songId: '91' };
  await client.podcastLikedPage(epoch, widerQuery);
  await client.podcastLikedCount(epoch);
  await client.podcastLikedPageProgram(epoch, widerTarget);
  await client.downloadLikedPageVoicePrepare(epoch, widerTarget);
  assert.deepEqual(calls, [
    ['podcast_liked_page', { epoch, query }],
    ['podcast_liked_count', { epoch }],
    ['podcast_liked_page_program', { epoch, target }],
    ['download_liked_page_voice_prepare', { epoch, target }],
  ]);
});

test('a direct adapter caller cannot consume a response for another epoch', async () => {
  const client = createPodcastLikedBridge(async <T>() => ({ epoch: 'other', value: 73 }) as T);
  await assert.rejects(client.podcastLikedCount(epoch), { code: 'stale' });
  await assert.rejects(
    client.downloadLikedPageVoicePrepare(epoch, { visit: 'v', revision: 'r', voiceId: '81' }),
    { code: 'stale' },
  );
});
