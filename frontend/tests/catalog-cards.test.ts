import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  albumCredits,
  albumCardDate,
  catalogResourceSelection,
} from '../src/services/catalogCards.ts';
import type { AlbumCard } from '../src/services/catalog.ts';
import type { CoreClient } from '../src/services/contracts.ts';
test('album cards follow official primary-credit order and date format without inventing missing fields', () => {
  const album = {
    artists: [
      { id: '1', name: 'A' },
      { id: '2', name: 'B' },
    ],
    primaryArtist: { id: '2', name: 'B' },
  } as AlbumCard;
  assert.deepEqual(
    albumCredits(album).map((artist) => artist.id),
    ['2', '1'],
  );
  assert.deepEqual(
    albumCredits({ ...album, primaryArtist: { id: '3', name: 'C' } }).map((artist) => artist.id),
    ['1', '2'],
  );
  assert.deepEqual(albumCredits({ ...album, artists: [] }), [album.primaryArtist]);
  assert.deepEqual(albumCredits({ ...album, artists: [], primaryArtist: null }), []);
  assert.equal(albumCardDate(new Date(2023, 10, 15).getTime()), '2023-11-15');
  for (const value of [null, 0, NaN, Infinity]) assert.equal(albumCardDate(value), '');
});
test('card playback uses fresh matching account/catalog revisions and skips missing songs', async () => {
  let epoch = '1',
    kind = 'album',
    id = '7',
    tracks = [{ missing: true }, { missing: false }];
  const calls: unknown[] = [];
  const core = {
    catalogPage: async (...args: unknown[]) => {
      calls.push(args);
      return { epoch, value: { query: { kind, id }, revision: 'fresh', page: { kind, tracks } } };
    },
  } as unknown as CoreClient;
  const resource = { kind: 'album' as const, id: '7' };
  assert.deepEqual(await catalogResourceSelection(core, '1', resource, 'hires'), {
    kind: 'catalog',
    pages: [{ query: resource, revision: 'fresh' }],
    index: 1,
    quality: 'hires',
  });
  assert.deepEqual(calls[0], ['1', resource, true]);
  id = '8';
  await assert.rejects(catalogResourceSelection(core, '1', resource, 'standard'), {
    code: 'unavailable',
  });
  id = '7';
  kind = 'artist';
  await assert.rejects(catalogResourceSelection(core, '1', resource, 'standard'), {
    code: 'unavailable',
  });
  kind = 'album';
  epoch = '2';
  await assert.rejects(catalogResourceSelection(core, '1', resource, 'standard'), {
    code: 'stale',
  });
  epoch = '1';
  tracks = [{ missing: true }];
  await assert.rejects(catalogResourceSelection(core, '1', resource, 'standard'), {
    code: 'unavailable',
  });
});
