import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  createLatestReleaseMemory,
  useLatestReleases,
} from '../src/composables/useLatestReleases.ts';
import type { AlbumCard } from '../src/services/catalog.ts';
import type { AccountData, TrackView } from '../src/services/contracts.ts';
import type { LatestReleasePage, LatestReleaseQuery } from '../src/services/latestReleases.ts';

const track = (id: string, missing = false) => ({ id, name: `歌曲 ${id}`, missing }) as TrackView;
const album = (id: string) => ({ id, name: `专辑 ${id}` }) as AlbumCard;
const songs = (id: string): LatestReleasePage => ({
  kind: 'songs',
  revision: `revision-${id}`,
  tracks: [track('900'), track(id), track('901', true)],
  playableIds: ['901', id, '902'],
});
const albumPage = (
  year: number,
  month: number,
  ids: string[],
  week: string[],
  next: { year: number; month: number; offset: number } | null,
): LatestReleasePage => ({
  kind: 'albums',
  year,
  month,
  week: week.map(album),
  albums: ids.map(album),
  next,
});
const tick = () => Promise.resolve();

test('latest songs fence old region/account responses and refresh the restored region', async () => {
  const memory = createLatestReleaseMemory();
  memory.connect('a');
  const reads: {
    epoch: string;
    query: LatestReleaseQuery;
    complete: (reply: AccountData<LatestReleasePage>) => void;
  }[] = [];
  const core = {
    latestReleases(epoch: string, query: LatestReleaseQuery) {
      return new Promise<AccountData<LatestReleasePage>>((complete) =>
        reads.push({ epoch, query, complete }),
      );
    },
  };
  const model = useLatestReleases(core, memory, () =>
    assert.fail('stale request cannot expire a session'),
  );
  model.open('a', 1, 'songs');
  assert.deepEqual(reads[0].query, { kind: 'songs', area: '0' });
  model.chooseSong('536870911');
  assert.deepEqual(reads[1].query, { kind: 'songs', area: '536870911' });
  reads[1].complete({ epoch: 'a', value: songs('7') });
  reads[0].complete({ epoch: 'a', value: songs('8') });
  await tick();
  assert.deepEqual(
    model.playable.value.map((row) => row.id),
    ['7'],
  );
  model.leave({ top: 118, focus: 'track:7' });
  assert.deepEqual(model.open('a', 1, 'songs'), { top: 118, focus: 'track:7' });
  assert.deepEqual(reads[2].query, { kind: 'songs', area: '536870911' });
  assert.equal(model.pending.value, true);
  assert.equal(model.songPage.value?.revision, 'revision-7');
  assert.deepEqual(
    model.playable.value.map((row) => row.id),
    ['7'],
  );
  reads[2].complete({ epoch: 'a', value: songs('9') });
  await tick();
  assert.equal(model.songPage.value?.revision, 'revision-9');
  assert.deepEqual(
    model.playable.value.map((row) => row.id),
    ['9'],
  );
  model.chooseSong('96');
  model.leave({ top: 0 });
  memory.connect('b');
  model.open('b', 1, 'songs');
  reads[3].complete({ epoch: 'a', value: songs('96') });
  await tick();
  assert.equal(model.songPage.value, undefined);
  assert.deepEqual(reads[4].query, { kind: 'songs', area: '0' });
  reads[4].complete({ epoch: 'b', value: songs('16') });
  await tick();
  assert.deepEqual(
    model.playable.value.map((row) => row.id),
    ['16'],
  );
});

test('a failed latest song revalidation keeps the table and retries the same region', async () => {
  const memory = createLatestReleaseMemory();
  memory.connect('a');
  const queries: LatestReleaseQuery[] = [];
  const core = {
    async latestReleases(epoch: string, query: LatestReleaseQuery) {
      queries.push(query);
      if (queries.length === 3) throw new Error('songs unavailable');
      return { epoch, value: songs(queries.length === 4 ? '11' : '7') };
    },
  };
  const model = useLatestReleases(core, memory, () => {});
  model.open('a', 1, 'songs');
  await tick();
  model.chooseSong('96');
  await tick();
  const original = model.songPage.value;
  model.leave({ top: 118 });
  model.open('a', 1, 'songs');
  await tick();
  assert.equal(model.songPage.value, original);
  assert.ok(model.error.value);
  await model.retry();
  assert.deepEqual(
    queries.map((query) => query.area),
    ['0', '96', '96', '96'],
  );
  assert.deepEqual(
    model.playable.value.map((row) => row.id),
    ['11'],
  );
  assert.equal(model.error.value, '');
});

test('latest albums retain week/month order, retry the same cursor and reset filters', async () => {
  const memory = createLatestReleaseMemory();
  memory.connect('a');
  const november = { year: 2026, month: 11, offset: 20 };
  const october = { year: 2026, month: 10, offset: 0 };
  const results = [
    albumPage(2026, 11, ['1'], ['3'], november),
    new Error('continuation unavailable'),
    albumPage(2026, 11, ['1', '2'], ['3'], october),
    albumPage(2026, 10, ['4'], [], october),
    albumPage(2026, 11, ['5'], [], null),
  ];
  const queries: LatestReleaseQuery[] = [];
  const core = {
    async latestReleases(epoch: string, query: LatestReleaseQuery) {
      queries.push(query);
      const value = results.shift()!;
      if (value instanceof Error) throw value;
      return { epoch, value };
    },
  };
  const model = useLatestReleases(core, memory, () => {});
  model.open('a', 2, 'albums');
  assert.equal(
    model.chooseAlbum('OTHER'),
    false,
    'official album region cannot replace a pending read',
  );
  await tick();
  assert.deepEqual(queries[0], { kind: 'albums', area: 'ALL', recommended: true, cursor: null });
  await model.load(true);
  assert.ok(model.error.value);
  assert.deepEqual(
    model.albumGroups.value.map((group) => group.title),
    ['本周新碟', '2026年11月'],
  );
  await model.retry();
  assert.deepEqual(queries[1], queries[2]);
  await model.load(true);
  assert.deepEqual(
    model.albumGroups.value.map((group) => group.albums.map((card) => card.id)),
    [['3'], ['1', '2'], ['4']],
  );
  assert.equal(model.next.value, null, 'repeated cursor does not loop');
  model.leave({ top: 640, focus: 'album:2026:10:album:4' });
  assert.deepEqual(model.open('a', 2, 'albums'), { top: 640, focus: 'album:2026:10:album:4' });
  assert.equal(queries.length, 4);
  assert.equal(model.chooseAlbum('OTHER', false), true);
  await tick();
  assert.deepEqual(queries[4], { kind: 'albums', area: 'OTHER', recommended: false, cursor: null });
  assert.deepEqual(
    model.albumGroups.value.map((group) => group.albums[0].id),
    ['5'],
  );
});
