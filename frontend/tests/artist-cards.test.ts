import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { artistCardMenu, artistLink } from '../src/services/artistCards.ts';
import { catalogMembershipObservation } from '../src/services/catalogMembership.ts';
import type { CatalogClient } from '../src/services/catalog.ts';

test('artist menus match recovered common and myFollow scene checks', () => {
  const oracle = JSON.parse(
    readFileSync(new URL('./fixtures/official-artist-card-menu.json', import.meta.url), 'utf8'),
  );
  for (const [index, scene] of [false, true].entries())
    assert.deepEqual(
      artistCardMenu(scene)
        .filter((item) => !item.separator)
        .map((item) => item.key),
      oracle.vectors[index].keys,
    );
  assert.equal(artistLink('13'), 'https://music.163.com/artist?id=13');
  for (const id of ['', '13&token=private', 'https://invalid.example/'])
    assert.equal(artistLink(id), '');
});

test('membership observations accept only fresh matching account, query and resource', async () => {
  let epoch = '1',
    kind = 'artist_dynamic',
    id = '13',
    page: object = { kind: 'artist_dynamic', followed: false };
  const calls: unknown[] = [];
  const core = {
    catalogPage: async (...args: unknown[]) => {
      calls.push(args);
      return { epoch, value: { query: { kind, id }, page } };
    },
  } as unknown as CatalogClient;
  const artist = { kind: 'artist' as const, id: '13' };
  assert.equal(await catalogMembershipObservation(core, '1', artist), false);
  assert.deepEqual(calls[0], ['1', { kind: 'artist_dynamic', id: '13' }, true]);
  page = { kind: 'artist_dynamic', followed: true };
  assert.equal(await catalogMembershipObservation(core, '1', artist), true);
  id = '14';
  await assert.rejects(catalogMembershipObservation(core, '1', artist), { code: 'stale' });
  id = '13';
  epoch = '2';
  await assert.rejects(catalogMembershipObservation(core, '1', artist), { code: 'stale' });
  epoch = '1';
  page = { kind: 'album', album: { id, subscribed: true } };
  await assert.rejects(catalogMembershipObservation(core, '1', artist), { code: 'stale' });
  kind = 'album';
  const album = { kind: 'album' as const, id };
  assert.equal(await catalogMembershipObservation(core, '1', album), true);
  page = { kind: 'album', album: { id, subscribed: null } };
  assert.equal(await catalogMembershipObservation(core, '1', album), null);
  page = { kind: 'album', album: { id: '14', subscribed: false } };
  await assert.rejects(catalogMembershipObservation(core, '1', album), { code: 'stale' });
});
