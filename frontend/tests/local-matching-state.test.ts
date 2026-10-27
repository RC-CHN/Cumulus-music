import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useLocalMatching } from '../src/composables/useLocalMatching.ts';
import type { AccountData, TrackPage, AccountView } from '../src/services/contracts.ts';
import type { LocalMatchView, MatchCommand } from '../src/services/localMatching.ts';
import type { LocalPage, LocalTrack } from '../src/services/local.ts';
const turn = () => new Promise((r) => setImmediate(r));
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
const account = (epoch: string, status: AccountView['status'] = 'authenticated'): AccountView => ({
  epoch,
  status,
  nickname: null,
  avatarUrl: null,
});
const row: LocalTrack = {
  revision: 'rev1',
  fileName: 'one.flac',
  folder: '/test',
  format: 'flac',
  cue: false,
  encrypted: false,
  unreadable: false,
  track: {
    id: 'local1',
    name: '歌曲',
    artists: ['歌手'],
    album: '专辑',
    durationMs: 1234,
    fee: 0,
    missing: false,
    local: true,
    coverUrl: null,
  },
};
const page: LocalPage = {
  revision: 'page1',
  tracks: [row],
  groups: [],
  roots: [],
  total: 1,
  playable: 1,
  offset: 0,
  more: false,
};
const empty: LocalMatchView = { job: null, links: [] };
const tracks: TrackPage = {
  revision: null,
  tracks: [{ ...row.track, id: '123', local: false }],
  total: 1,
  offset: 0,
  more: false,
  playlist: null,
};

test('search pagination resets for changed keywords and current session expiry refreshes the account', async () => {
  const requests: { keyword: string; offset: number }[] = [];
  let changed = 0;
  const model = useLocalMatching(
    {
      localMatchView: async (epoch) => ({ epoch, value: empty }),
      localMatchCommand: async () => {},
      searchSongs: async (keyword, page) => {
        requests.push({ keyword, offset: page.offset });
        if (keyword === 'expired') throw { code: 'session_expired', message: '需要登录' };
        return { epoch: '1', value: { ...tracks, offset: page.offset } };
      },
    },
    () => {
      changed++;
    },
  );
  model.connect(account('1'));
  model.setPage(page);
  await turn();
  model.open(row);
  await model.find(0);
  await model.find(20);
  model.keyword.value = 'new';
  await model.find(40);
  assert.deepEqual(
    requests.map((r) => r.offset),
    [0, 20, 0],
  );
  model.keyword.value = 'expired';
  await model.find();
  assert.equal(changed, 1);
  assert.equal(model.searchError.value, '需要登录');
  model.dispose();
});

test('account generations discard late mappings and searches while device page remains usable', async () => {
  const read = deferred<AccountData<LocalMatchView>>(),
    search = deferred<AccountData<TrackPage>>();
  let calls = 0;
  const model = useLocalMatching({
    localMatchView: async (epoch) => {
      calls++;
      return epoch === '1' ? read.promise : { epoch, value: empty };
    },
    localMatchCommand: async () => {},
    searchSongs: () => search.promise,
  });
  model.connect(account('1'));
  model.setPage(page);
  model.open(row);
  const pending = model.find();
  model.connect(account('2'));
  await turn();
  read.resolve({
    epoch: '1',
    value: {
      job: null,
      links: [{ id: 'local1', revision: 'rev1', songId: '123', track: null, source: 'search' }],
    },
  });
  search.resolve({ epoch: '1', value: tracks });
  await pending;
  await turn();
  assert.equal(model.link(row), undefined);
  assert.equal(model.results.value, undefined);
  assert.equal(model.target.value, undefined);
  model.connect(account('3', 'signed_out'));
  model.setPage(page);
  await turn();
  assert.equal(calls, 2);
  assert.equal(model.ready.value, false);
  model.dispose();
});

test('mapping display requires matching file revision and commands capture selected item', async () => {
  const writes: MatchCommand[] = [];
  const model = useLocalMatching({
    localMatchView: async (epoch) => ({
      epoch,
      value: {
        job: null,
        links: [{ id: 'local1', revision: 'rev1', songId: '123', source: 'manual', track: null }],
      },
    }),
    localMatchCommand: async (_epoch, c) => {
      writes.push(c);
    },
    searchSongs: async () => ({ epoch: '1', value: tracks }),
  });
  model.connect(account('1'));
  model.setPage(page);
  await turn();
  assert.equal(model.link(row)?.songId, '123');
  assert.equal(model.link({ ...row, revision: 'new' }), undefined);
  model.open(row);
  await model.find();
  await model.bind('123');
  assert.deepEqual(writes[0], { kind: 'bind', id: 'local1', revision: 'rev1', song_id: '123' });
  model.open(row);
  await model.clear();
  assert.deepEqual(writes[1], { kind: 'clear', id: 'local1', revision: 'rev1' });
  await model.start();
  assert.equal(writes[2]?.kind, 'start');
  if (writes[2]?.kind === 'start') {
    assert.equal(writes[2].query.offset, 0);
    assert.equal(writes[2].query.keyword, '');
  }
  model.dispose();
});

test('pause/resume/cancel use durable job identity; active matching blocks manual dialog', async () => {
  const writes: MatchCommand[] = [];
  const model = useLocalMatching({
    localMatchView: async (epoch) => ({
      epoch,
      value: {
        links: [],
        job: {
          id: 'job1',
          phase: 'running',
          total: 103,
          processed: 50,
          matched: 49,
          unmatched: 1,
          skipped: 0,
          error: null,
          revision: 1,
        },
      },
    }),
    localMatchCommand: async (_e, c) => {
      writes.push(c);
    },
    searchSongs: async () => ({ epoch: '1', value: tracks }),
  });
  model.connect(account('1'));
  model.setPage(page);
  await turn();
  model.open(row);
  assert.equal(model.target.value, undefined);
  for (const action of ['pause', 'resume', 'cancel'] as const) await model.control(action);
  assert.deepEqual(
    writes,
    ['pause', 'resume', 'cancel'].map((action) => ({ kind: 'control', id: 'job1', action })),
  );
  model.dispose();
});

test('closed dialog and disposed model ignore late search and mutation results', async () => {
  const search = deferred<AccountData<TrackPage>>(),
    write = deferred<void>();
  const model = useLocalMatching({
    localMatchView: async (epoch) => ({ epoch, value: empty }),
    localMatchCommand: () => write.promise,
    searchSongs: () => search.promise,
  });
  model.connect(account('1'));
  model.setPage(page);
  await turn();
  model.open(row);
  const pending = model.find();
  model.close();
  search.resolve({ epoch: '1', value: tracks });
  await pending;
  assert.equal(model.results.value, undefined);
  model.open(row);
  const saving = model.bind('123');
  model.connect(account('2'));
  model.open({ ...row, track: { ...row.track, id: 'new' } });
  write.resolve();
  await saving;
  assert.equal(model.target.value?.track.id, 'new');
  model.dispose();
});
