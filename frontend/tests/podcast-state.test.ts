import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePodcasts } from '../src/composables/usePodcasts.ts';
import type {
  PodcastClient,
  PodcastPage,
  PodcastState,
  PodcastMutation,
  PodcastSubscriptionRecommendation,
} from '../src/services/podcast.ts';
import type { AccountData } from '../src/services/contracts.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
const operation = (phase: PodcastMutation['phase'] = 'uncertain'): PodcastMutation => ({
  action: 'like',
  id: '81',
  desired: true,
  observed: false,
  phase,
  acknowledged: false,
});
const page = (epoch = '1'): PodcastPage => ({
  epoch,
  scope: { kind: 'liked' },
  revision: 'r1',
  cards: [],
  episodes: [],
  total: 205,
  offset: 0,
  remoteTotal: 205,
  radio: null,
  likedIds: ['81'],
  resumeEnabled: null,
  operations: [],
});
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function client(extra: Partial<PodcastClient> = {}): PodcastClient {
  return {
    podcastPage: async (e) => page(e),
    podcastSubscriptionRecommendations: async (e) => ({ epoch: e, value: [] }),
    podcastClearSubscribedBadge: async () => {},
    podcastRadioOrder: async (e) => ({ epoch: e, value: null }),
    podcastState: async (e) => ({ epoch: e, value: { likedIds: [], operations: [] } }),
    podcastCommand: async (e) => ({ epoch: e, value: operation('confirmed') }),
    ...extra,
  };
}
test('late account page and persisted mutation facts never populate the new account', async () => {
  const held = deferred<PodcastPage>(),
    state = deferred<AccountData<PodcastState>>();
  const m = usePodcasts(
    client({ podcastPage: () => held.promise, podcastState: () => state.promise }),
  );
  m.connect(account());
  const request = m.load();
  m.connect(account('2'));
  held.resolve(page());
  state.resolve({ epoch: '1', value: { likedIds: ['81'], operations: [operation()] } });
  await request;
  await Promise.resolve();
  assert.equal(m.page.value, undefined);
  assert.equal(m.liked.value.size, 0);
  assert.equal(m.operations.value.length, 0);
  assert.equal(m.loading.value, false);
});
test('unknown remote write exposes durable readback while offline and never repeats the write', async () => {
  let writes = 0,
    checks = 0,
    dirty = false;
  const m = usePodcasts(
    client({
      podcastPage: async () => {
        throw { code: 'network', message: 'offline' };
      },
      podcastState: async (e, refresh) => {
        if (refresh) throw { code: 'network', message: 'offline' };
        return {
          epoch: e,
          value: { likedIds: dirty ? null : [], operations: dirty ? [operation()] : [] },
        };
      },
      podcastCommand: async (e, c) => {
        if (c.kind === 'change') {
          writes++;
          dirty = true;
          throw { code: 'network', message: 'lost reply' };
        }
        checks++;
        dirty = false;
        return { epoch: e, value: operation('confirmed') };
      },
    }),
  );
  m.connect(account());
  await m.state();
  await m.change('like', '81', true);
  assert.equal(m.operations.value.length, 1);
  assert.equal(m.likesKnown.value, false);
  assert.equal(m.error.value, 'lost reply');
  await m.change('like', '81', true);
  assert.equal(writes, 1);
  await m.command({ kind: 'reconcile', action: 'like', id: '81' });
  assert.equal(checks, 1);
  assert.equal(writes, 1);
  assert.equal(m.operations.value.length, 0);
});
test('old page cannot overwrite a completed mutation or stop a later load indicator', async () => {
  const held = deferred<PodcastPage>();
  let calls = 0;
  const m = usePodcasts(
    client({
      podcastPage: async () => (++calls === 1 ? held.promise : page()),
      podcastState: async (e) => ({ epoch: e, value: { likedIds: ['82'], operations: [] } }),
    }),
  );
  m.connect(account());
  const loading = m.load();
  await m.change('like', '82', true);
  held.resolve(page());
  await loading;
  assert.deepEqual([...m.liked.value], ['82']);
  assert.equal(m.loading.value, false);
});
test('pagination sends the native revision and returning from detail restores list query', async () => {
  const seen: { offset: number; revision: string | null; kind: string }[] = [];
  const m = usePodcasts(
    client({
      podcastPage: async (e, q) => {
        seen.push({ offset: q.offset, revision: q.revision, kind: q.scope.kind });
        return { ...page(e), scope: q.scope, offset: q.offset };
      },
    }),
  );
  m.connect(account());
  await m.load();
  await m.paginate(150);
  assert.deepEqual(seen.at(-1), { offset: 150, revision: 'r1', kind: 'podcasts' });
  m.navigate({ kind: 'radio', id: '7', ascending: true });
  assert.equal(m.canBack.value, true);
  m.back();
  assert.equal(m.query.value.offset, 150);
  assert.equal(m.query.value.scope.kind, 'podcasts');
  assert.equal(m.query.value.revision, null);
});
test('switching accounts while a write is pending discards its notices and followup requests', async () => {
  const held = deferred<AccountData<PodcastMutation>>();
  let reads = 0;
  const m = usePodcasts(
    client({
      podcastCommand: () => held.promise,
      podcastPage: async (e) => {
        reads++;
        return page(e);
      },
    }),
  );
  m.connect(account());
  const write = m.change('like', '81', true);
  m.connect(account('2'));
  held.resolve({ epoch: '1', value: operation('confirmed') });
  await write;
  assert.equal(m.notice.value, '');
  assert.equal(m.busy.value, false);
  assert.equal(reads, 0);
});

test('radio references preserve large IDs and reject other resources and hostile URLs', async () => {
  const { podcastReference } = await import('../src/services/podcast.ts');
  assert.equal(podcastReference('9007199254740993'), '9007199254740993');
  assert.equal(
    podcastReference('https://music.163.com/#/djradio?id=9007199254740993'),
    '9007199254740993',
  );
  assert.equal(podcastReference('https://music.163.com/djradio?id=81'), '81');
  for (const value of [
    '0',
    '-1',
    '1e3',
    'https://music.163.com/song?id=81',
    'https://music.163.com.evil.test/djradio?id=81',
    'https://user@music.163.com/djradio?id=81',
    'https://music.163.com/djradio?id=81&id=82',
  ])
    assert.equal(podcastReference(value), null);
  const m = usePodcasts(client());
  m.connect(account());
  m.open('https://music.163.com/djradio?id=81');
  assert.deepEqual(m.query.value.scope, { kind: 'radio', id: '81', ascending: true });
  m.open('https://example.test/?id=9');
  assert.match(m.error.value, /有效/);
  assert.equal(m.query.value.scope.kind, 'radio');
});

function detail(epoch: string, q: Parameters<PodcastClient['podcastPage']>[1]): PodcastPage {
  if (q.scope.kind !== 'radio') return page(epoch);
  return {
    ...page(epoch),
    scope: q.scope,
    offset: q.offset,
    revision: `radio:${q.scope.id}:${q.scope.ascending}`,
    radio: {
      id: q.scope.id,
      name: '合成播客',
      coverUrl: null,
      creator: '',
      description: '',
      programCount: 2,
      subscribed: false,
      feeType: 0,
      unavailable: false,
    },
  };
}

test('only an explicit radio sort writes; retries, reload and Back retain the committed order', async () => {
  const writes: unknown[] = [];
  const orders = new Map<string, boolean>();
  let fail = true;
  const api = client({
    podcastRadioOrder: async (epoch, value) => {
      const key = `${epoch}:${value.kind === 'read' ? value.id : value.scope.id}`;
      if (value.kind === 'read') return { epoch, value: orders.get(key) ?? null };
      writes.push(value);
      orders.set(key, value.ascending);
      return { epoch, value: value.ascending };
    },
    podcastPage: async (epoch, query) => {
      if (query.scope.kind === 'radio' && !query.scope.ascending && fail)
        throw { code: 'network', message: '列表暂时不可用' };
      return detail(epoch, query);
    },
  });
  const m = usePodcasts(api);
  m.connect(account());
  m.show(true);
  const radio = { kind: 'radio', id: '7', ascending: true } as const;
  m.navigate(radio);
  await new Promise<void>((done) => setImmediate(done));
  await m.search('合成');
  await m.paginate(50);
  await m.toggleRadioOrder();
  assert.equal(m.error.value, '列表暂时不可用');
  assert.deepEqual(
    m.page.value?.scope,
    radio,
    'failed read keeps the displayed list and scope together',
  );
  assert.deepEqual(writes, [
    { kind: 'set', scope: radio, revision: 'radio:7:true', ascending: false },
  ]);
  fail = false;
  await m.load(true);
  assert.deepEqual(m.page.value?.scope, { ...radio, ascending: false });
  assert.equal(m.query.value.keyword, '合成');
  assert.equal(m.query.value.offset, 50);
  m.back();
  await new Promise<void>((done) => setImmediate(done));
  assert.equal(m.page.value?.scope.kind, 'liked');

  const reopened = usePodcasts(api);
  reopened.connect(account());
  reopened.navigate(radio);
  await reopened.load(true);
  assert.deepEqual(reopened.page.value?.scope, { ...radio, ascending: false });
  reopened.connect(account('2'));
  reopened.navigate(radio);
  await reopened.load(true);
  assert.deepEqual(reopened.page.value?.scope, radio);
  assert.equal(writes.length, 1);
});

test('late saved-order read cannot issue a request for the abandoned radio', async () => {
  const held = deferred<AccountData<boolean | null>>();
  const radioReads: string[] = [];
  const m = usePodcasts(
    client({
      podcastRadioOrder: async (epoch, query) => {
        assert.equal(query.kind, 'read');
        return query.id === '7' ? held.promise : { epoch, value: true };
      },
      podcastPage: async (epoch, query) => {
        if (query.scope.kind === 'radio') radioReads.push(query.scope.id);
        return detail(epoch, query);
      },
    }),
  );
  m.connect(account());
  m.show(true);
  m.navigate({ kind: 'radio', id: '7', ascending: true });
  m.navigate({ kind: 'radio', id: '8', ascending: false });
  await new Promise<void>((done) => setImmediate(done));
  held.resolve({ epoch: '1', value: false });
  await new Promise<void>((done) => setImmediate(done));
  assert.deepEqual(radioReads, ['8']);
  assert.deepEqual(m.page.value?.scope, { kind: 'radio', id: '8', ascending: true });
  assert.equal(m.loading.value, false);
});

test('only an explicit positive subscribed card opens and consumes its exact native receipt', async () => {
  const clears: string[][] = [];
  const reads: string[] = [];
  const m = usePodcasts(
    client({
      podcastPage: async (owner, q) => {
        reads.push(q.scope.kind);
        if (q.scope.kind !== 'podcasts') return { ...page(owner), scope: q.scope };
        return {
          ...page(owner),
          scope: q.scope,
          subscribedBadgeReceipt: `subscribed-${owner}`,
          cards: [
            {
              id: '7',
              name: 'Observed',
              coverUrl: null,
              creator: '',
              description: '',
              programCount: 50,
              subscribed: true,
              feeType: 0,
              unavailable: false,
              newEpisodeCount: 108,
            },
          ],
        };
      },
      podcastClearSubscribedBadge: async (...args) => {
        clears.push(args);
        throw new Error('ambiguous synthetic response');
      },
    }),
  );
  m.connect(account());
  m.show(true);
  await m.load(true);
  assert.deepEqual(clears, [], 'mount/read cannot clear');
  m.open('7');
  assert.deepEqual(clears, [], 'typed input deep link cannot clear');
  assert.equal(m.query.value.scope.kind, 'radio');
  m.back();
  await m.load(true);
  m.openCard('7');
  assert.deepEqual(clears, [['1', 'subscribed-1', '7']]);
  assert.equal(m.query.value.scope.kind, 'radio');
  await m.load(true);
  assert.equal(m.page.value?.scope.kind, 'radio');
  await Promise.resolve();
  assert.equal(m.error.value, '', 'ordinary detail survives an unknown clear result');
  assert.equal(reads.includes('radio'), true);
  assert.deepEqual(clears, [['1', 'subscribed-1', '7']], 'generic reload never retries');
  m.dispose();
});

const recommended = (id: string): PodcastSubscriptionRecommendation => ({
  id,
  name: '合成推荐',
  coverUrl: null,
  playCount: 10,
  unavailable: false,
});

test('recommendations load once per active My Podcasts visit and use the total podcast count', async () => {
  const reads: string[] = [];
  const clear: string[] = [];
  let total = 50;
  const api = client({
    podcastSubscriptionRecommendations: async (e) => {
      reads.push(e);
      return { epoch: e, value: [recommended('81')] };
    },
    podcastPage: async (e, request) => ({
      ...page(e),
      scope: request.scope,
      // The filtered row count is not the subscriptions total.
      total: 0,
      remoteTotal: total,
      subscribedBadgeReceipt: 'fixture',
      cards: [
        {
          ...recommended('81'),
          creator: '',
          description: '',
          programCount: 1,
          subscribed: true,
          feeType: 0,
          newEpisodeCount: 3,
        },
      ],
    }),
    podcastClearSubscribedBadge: async (_, __, id) => {
      clear.push(id);
    },
  });
  const model = usePodcasts(api);
  model.connect(account());
  assert.deepEqual(reads, []);
  model.show(true);
  await model.load(true);
  assert.deepEqual(reads, ['1']);
  assert.deepEqual(model.recommendedCards.value, []);
  total = 49;
  await model.load(true);
  assert.deepEqual(
    model.recommendedCards.value.map((c) => c.id),
    ['81'],
  );
  model.navigate({ kind: 'audiobooks' });
  await model.load(true);
  assert.deepEqual(model.recommendedCards.value, []);
  model.navigate({ kind: 'podcasts' });
  await model.load(true);
  model.openRecommended('not-on-shelf');
  assert.equal(model.query.value.scope.kind, 'podcasts');
  model.openRecommended('81');
  assert.deepEqual(model.query.value.scope, { kind: 'radio', id: '81', ascending: true });
  assert.deepEqual(clear, [], 'same-ID subscription badge is not consumed');
  model.back();
  await model.load(true);
  assert.equal(model.recommendedCards.value[0]?.id, '81');
  model.showRecent();
  assert.deepEqual(model.recommendedCards.value, []);
  assert.deepEqual(reads, ['1']);
  model.show(false);
  model.show(true);
  assert.deepEqual(reads, ['1', '1']);
});

test('late recommendation from inactive visit or old account cannot populate the shelf', async () => {
  const oldVisit = deferred<AccountData<PodcastSubscriptionRecommendation[]>>();
  const oldAccount = deferred<AccountData<PodcastSubscriptionRecommendation[]>>();
  const reads: string[] = [];
  const model = usePodcasts(
    client({
      podcastSubscriptionRecommendations: async (e) => {
        reads.push(e);
        return reads.length === 1
          ? oldVisit.promise
          : reads.length === 2
            ? oldAccount.promise
            : { epoch: e, value: [recommended('93')] };
      },
      podcastPage: async (e, request) => ({ ...page(e), scope: request.scope, remoteTotal: 0 }),
    }),
  );
  model.connect(account());
  model.show(true);
  model.show(false);
  model.show(true);
  model.connect(account('2'));
  await model.load(true);
  oldVisit.resolve({ epoch: '1', value: [recommended('91')] });
  oldAccount.resolve({ epoch: '1', value: [recommended('92')] });
  await Promise.all([oldVisit.promise, oldAccount.promise]);
  await Promise.resolve();
  assert.deepEqual(reads, ['1', '1', '2']);
  assert.deepEqual(
    model.recommendedCards.value.map((c) => c.id),
    ['93'],
  );
});

test('opening a radio from outside My Podcasts does not read subscription recommendations', async () => {
  const reads: string[] = [];
  const model = usePodcasts(
    client({
      podcastSubscriptionRecommendations: async (e) => {
        reads.push(e);
        return { epoch: e, value: [recommended('81')] };
      },
      podcastPage: async (e, request) => ({ ...page(e), scope: request.scope, remoteTotal: 0 }),
    }),
  );
  model.connect(account());
  model.navigate({ kind: 'radio', id: '81', ascending: true });
  model.show(true);
  await model.load(true);
  assert.deepEqual(reads, []);
  model.back();
  await model.load(true);
  assert.deepEqual(reads, ['1']);
});
