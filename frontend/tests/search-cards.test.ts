import { test } from 'node:test';
import assert from 'node:assert/strict';
import { searchCardSelection, searchCardIsCurrent } from '../src/services/searchCards.ts';
import type { CoreClient } from '../src/services/contracts.ts';
import type { SearchItem } from '../src/services/search.ts';
const artist = { kind: 'artist', artist: { id: '12' } } as SearchItem;
const voice = {
  kind: 'voice',
  voice: { id: '21', radioId: '7', unavailable: false },
} as SearchItem;
test('cards select authoritative catalog and radio revisions, not cached search titles', async () => {
  const calls: unknown[] = [];
  const core = {
    catalogPage: async (...args: unknown[]) => {
      calls.push(args);
      return {
        epoch: '1',
        value: {
          query: { kind: 'artist', id: '12' },
          revision: 'fresh',
          page: {
            kind: 'artist',
            tracks: [
              { id: '1', missing: true },
              { id: '2', missing: false },
            ],
          },
        },
      };
    },
    podcastPage: async (...args: unknown[]) => {
      calls.push(args);
      return { epoch: '1', revision: 'radio-fresh', episodes: [] };
    },
  } as unknown as CoreClient;
  assert.deepEqual(await searchCardSelection(core, '1', artist, 'lossless'), {
    kind: 'catalog',
    pages: [{ query: { kind: 'artist', id: '12' }, revision: 'fresh' }],
    index: 1,
    quality: 'lossless',
  });
  assert.deepEqual(await searchCardSelection(core, '1', voice, 'standard'), {
    kind: 'podcast',
    scope: { kind: 'radio', id: '7', ascending: true },
    revision: 'radio-fresh',
    start_id: '21',
  });
  assert.equal((calls[0] as unknown[])[2], true);
  assert.equal((calls[1] as unknown[])[2], true);
  assert.equal(searchCardIsCurrent(artist, { kind: 'album', id: '12' }), false);
  assert.equal(searchCardIsCurrent(artist, { kind: 'artist', id: '12' }), true);
  assert.equal(searchCardIsCurrent(voice, null, '21'), true);
});
test('missing catalog data and stale radio snapshots cannot become playback selections', async () => {
  const core = {
    catalogPage: async () => ({ epoch: '1', value: null }),
    podcastPage: async () => ({ epoch: 'old', revision: 'stale' }),
  } as unknown as CoreClient;
  await assert.rejects(searchCardSelection(core, '1', artist, 'standard'), { code: 'unavailable' });
  await assert.rejects(searchCardSelection(core, '1', voice, 'standard'), { code: 'stale' });
});
