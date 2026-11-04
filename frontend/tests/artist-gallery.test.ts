import { test } from 'node:test';
import assert from 'node:assert/strict';
import { shallowRef } from 'vue';
import { useArtistGallery } from '../src/composables/useArtistGallery.ts';
import type { ArtistGalleryPage, CatalogClient } from '../src/services/catalog.ts';
import type { AccountData, AccountView } from '../src/services/contracts.ts';

const result = (
  epoch: string,
  id: string,
  nextOffset: number | null,
): AccountData<ArtistGalleryPage> => ({
  epoch,
  value: {
    artists: [
      {
        id,
        name: id,
        aliases: [],
        coverUrl: null,
        description: '',
        songCount: 1,
        albumCount: 0,
        mvCount: null,
      },
    ],
    nextOffset,
    capped: false,
  },
});

test('gallery keeps the selected filters and prefix, rejects late reads and resets on account switch', async () => {
  const account = shallowRef<AccountView>({
    epoch: '1',
    status: 'authenticated',
    nickname: null,
    avatarUrl: null,
  });
  const reads: Array<[string, number, number]> = [];
  let resolvePrior!: (reply: AccountData<ArtistGalleryPage>) => void;
  let resolvePage!: (reply: AccountData<ArtistGalleryPage>) => void;
  let fail = true;
  const core: Pick<CatalogClient, 'artistGallery'> = {
    artistGallery(epoch, query) {
      reads.push([epoch, query.area, query.offset]);
      if (query.area === -1) return new Promise((resolve) => (resolvePrior = resolve));
      if (query.offset === 0) return Promise.resolve(result(epoch, 'first', 20));
      if (fail) {
        fail = false;
        return Promise.reject({ code: 'transport', message: '续页中断' });
      }
      return new Promise((resolve) => (resolvePage = resolve));
    },
  };
  const model = useArtistGallery(
    core,
    () => account.value,
    () => assert.fail('no expiry'),
  );
  const prior = model.load();
  model.select({ area: 7 });
  resolvePrior(result('1', 'late', null));
  await prior;
  assert.deepEqual(
    model.page.value.artists.map((row) => row.id),
    ['first'],
  );
  await model.load();
  assert.equal(model.error.value, '续页中断');
  assert.equal(model.page.value.nextOffset, 20);
  assert.deepEqual(
    model.page.value.artists.map((row) => row.id),
    ['first'],
  );
  const page = model.load();
  account.value = { ...account.value, epoch: '2' };
  resolvePage(result('1', 'second', null));
  await page;
  assert.equal(model.filter.value.area, -1);
  assert.deepEqual(model.page.value.artists, []);
  assert.equal(model.pending.value, false);
  assert.deepEqual(reads, [
    ['1', -1, 0],
    ['1', 7, 0],
    ['1', 7, 20],
    ['1', 7, 20],
  ]);
});
