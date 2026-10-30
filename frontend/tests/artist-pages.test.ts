import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ref } from 'vue';
import { useArtistPages } from '../src/composables/useArtistPages.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type { ArtistTab, CatalogQuery, CatalogView } from '../src/services/catalog.ts';
const account: AccountView = {
  epoch: 'one',
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
};
const songs: CatalogQuery = { kind: 'artist_songs', id: '12', offset: 0, limit: 20 };
const albums: CatalogQuery = { kind: 'artist_albums', id: '12', offset: 0, limit: 20 };
function view(query: CatalogQuery): CatalogView {
  if (query.kind !== 'artist_songs' && query.kind !== 'artist_albums') throw Error('fixture query');
  return {
    query: { ...query },
    fetchedAt: 1,
    cached: false,
    revision: `fixture-${query.id}-${query.offset}`,
    page:
      query.kind === 'artist_songs'
        ? {
            kind: 'artist_songs',
            offset: query.offset,
            total: 55,
            more: query.offset < 40,
            tracks: [],
          }
        : {
            kind: 'artist_albums',
            offset: query.offset,
            total: 62,
            more: query.offset < 60,
            albums: [],
          },
  };
}
const reply = (query: CatalogQuery, epoch = 'one') => ({ epoch, value: view(query) });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));
test('artist tab returns keep complete prefixes without replacing them with page one', async () => {
  const tab = ref<ArtistTab>('all'),
    calls: CatalogQuery[] = [];
  const model = useArtistPages(tab, {
    catalogPage: async (_e, q) => {
      calls.push({ ...q });
      return reply(q);
    },
  });
  model.connect(account);
  await model.open(songs);
  await model.more();
  await model.more();
  assert.deepEqual(
    model.pages.value.map((p) => 'offset' in p.query && p.query.offset),
    [0, 20, 40],
  );
  tab.value = 'albums';
  await model.open(albums);
  await model.more();
  const count = calls.length;
  tab.value = 'all';
  await model.open(songs);
  assert.equal(model.pages.value.length, 3);
  tab.value = 'albums';
  await model.open(albums);
  assert.equal(model.pages.value.length, 2);
  assert.equal(calls.length, count);
  model.dispose();
});
test('an inactive tab response cannot overwrite the current album panel', async () => {
  const tab = ref<ArtistTab>('all'),
    held = deferred<ReturnType<typeof reply>>();
  const model = useArtistPages(tab, {
    catalogPage: async (_e, q, refresh) =>
      q.kind === 'artist_songs' && refresh ? held.promise : reply(q),
  });
  model.connect(account);
  const pending = model.open(songs);
  await tick();
  tab.value = 'albums';
  await model.open(albums);
  held.resolve(reply(songs));
  await pending;
  assert.equal(model.pages.value[0].page.kind, 'artist_albums');
  tab.value = 'all';
  await model.open(songs);
  assert.equal(model.pages.value[0].page.kind, 'artist_songs');
  model.dispose();
});
test('failed refresh retains continuation pages and requires an explicit retry after returning', async () => {
  const tab = ref<ArtistTab>('all');
  let fail = false,
    reads = 0;
  const model = useArtistPages(tab, {
    catalogPage: async (_e, q) => {
      reads++;
      if (fail && q.kind === 'artist_songs') throw { code: 'network', message: '合成离线' };
      return reply(q);
    },
  });
  model.connect(account);
  await model.open(songs);
  await model.more();
  fail = true;
  await model.open(songs, true);
  assert.equal(model.pages.value.length, 2);
  assert.equal(model.error.value, '合成离线');
  tab.value = 'albums';
  await model.open(albums);
  const count = reads;
  tab.value = 'all';
  await model.open(songs);
  assert.equal(reads, count);
  assert.equal(model.error.value, '合成离线');
  assert.equal(model.pages.value.length, 2);
  fail = false;
  await model.open(songs, true);
  assert.equal(model.pages.value.length, 1);
  assert.equal(model.error.value, '');
  model.dispose();
});
test('clearing a resource invalidates its late reads even when the next resource uses the same account', async () => {
  const tab = ref<ArtistTab>('all'),
    held = deferred<ReturnType<typeof reply>>();
  let requests = 0;
  const model = useArtistPages(tab, {
    catalogPage: async (_e, q, refresh) => {
      if (refresh && ++requests === 1) return held.promise;
      return reply(q);
    },
  });
  model.connect(account);
  const pending = model.open(songs);
  await tick();
  model.clear();
  assert.deepEqual(model.pages.value, []);
  assert.equal(model.loading.value, false);
  const other = { ...songs, id: '13' };
  await model.open(other);
  held.resolve(reply(songs));
  await pending;
  assert.equal(model.pages.value[0].query.id, '13');
  model.dispose();
});
test('account changes discard all tab prefixes and fence old account replies', async () => {
  const tab = ref<ArtistTab>('all'),
    held = deferred<ReturnType<typeof reply>>();
  const model = useArtistPages(tab, {
    catalogPage: async (e, q, refresh) => (e === 'one' && refresh ? held.promise : reply(q, e)),
  });
  model.connect(account);
  const pending = model.open(songs);
  await tick();
  model.connect({ ...account, epoch: 'two' });
  await model.open(songs);
  held.resolve(reply(songs));
  await pending;
  assert.equal(model.pages.value.length, 1);
  assert.equal(model.error.value, '');
  tab.value = 'albums';
  await model.open(albums);
  assert.equal(model.pages.value[0].page.kind, 'artist_albums');
  model.dispose();
});

test('retry after a failed first-page refresh never loads the next continuation instead', async () => {
  const tab = ref<ArtistTab>('all');
  let fail = false;
  const offsets: number[] = [];
  const model = useArtistPages(tab, {
    catalogPage: async (_e, q) => {
      if ('offset' in q) offsets.push(q.offset);
      if (fail) throw { code: 'network', message: '合成离线' };
      return reply(q);
    },
  });
  model.connect(account);
  await model.open(songs);
  await model.more();
  fail = true;
  await model.open(songs, true);
  fail = false;
  await model.retry();
  assert.equal(offsets.at(-1), 0);
  assert.equal(model.pages.value.length, 1);
  model.dispose();
});
test('retry after a failed continuation appends that page exactly once', async () => {
  const tab = ref<ArtistTab>('all');
  let fail = true;
  const offsets: number[] = [];
  const model = useArtistPages(tab, {
    catalogPage: async (_e, q) => {
      if ('offset' in q) offsets.push(q.offset);
      if (q.kind === 'artist_songs' && q.offset === 20 && fail)
        throw { code: 'network', message: '合成离线' };
      return reply(q);
    },
  });
  model.connect(account);
  await model.open(songs);
  await model.more();
  assert.equal(model.pages.value.length, 1);
  fail = false;
  await model.retry();
  assert.equal(offsets.at(-1), 20);
  assert.equal(model.pages.value.length, 2);
  model.dispose();
});
