import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePodcasts } from '../src/composables/usePodcasts.ts';
import type {
  PodcastClient,
  PodcastPage,
  PodcastState,
  PodcastMutation,
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
