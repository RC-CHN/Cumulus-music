import assert from 'node:assert/strict';
import { test } from 'node:test';
import { usePlaylistCreator } from '../src/composables/usePlaylistCreator.ts';
import type { AccountData, PlaylistCreatorProfile } from '../src/services/contracts.ts';

const profile = (id: string): PlaylistCreatorProfile => ({
  id,
  nickname: id,
  avatarUrl: null,
  signature: '',
  created: [],
  collected: [],
});

test('creator profile retains the selected group when returning, but rejects stale account results', async () => {
  let release!: (value: AccountData<PlaylistCreatorProfile>) => void;
  const reads: string[] = [];
  const model = usePlaylistCreator({
    playlistCreatorProfile: (epoch, id) => {
      reads.push(`${epoch}:${id}`);
      if (id === '8')
        return new Promise((resolve) => {
          release = resolve;
        });
      return Promise.resolve({ epoch, value: profile(id) });
    },
  });
  await model.enter('old', '7');
  model.select('collected');
  model.enter('old', null);
  await model.enter('old', '7');
  assert.equal(model.group.value, 'collected');
  assert.deepEqual(reads, ['old:7']);
  const delayed = model.enter('old', '8');
  await model.enter('new', '7');
  release({ epoch: 'old', value: profile('8') });
  await delayed;
  assert.equal(model.profile.value?.id, '7');
  assert.equal(model.group.value, 'created');
  assert.deepEqual(reads, ['old:7', 'old:8', 'new:7']);
});

test('the rank entry occupies one slot in the 20-item created page and never shifts collected', async () => {
  const playlist = (id: number): PlaylistCreatorProfile['created'][number] => ({
    id: String(id),
    name: String(id),
    creator: '',
    creatorId: null,
    creatorAvatarUrl: null,
    coverUrl: null,
    description: '',
    trackCount: 0,
    owned: true,
    favorite: false,
    private: false,
  });
  const model = usePlaylistCreator({
    playlistCreatorProfile: async (epoch, id) => ({
      epoch,
      value: {
        ...profile(id),
        listeningRank: { isSelf: true, listenSongs: 0 },
        created: Array.from({ length: 40 }, (_, index) => playlist(index + 1)),
        collected: Array.from({ length: 21 }, (_, index) => playlist(index + 100)),
      },
    }),
  });
  await model.enter('epoch', '9');
  const rendered = () => model.cards.value.map((row) => Number(row.id));
  assert.equal(model.pages.value, 3);
  assert.deepEqual(
    rendered(),
    Array.from({ length: 19 }, (_, index) => index + 1),
  );
  model.page.value = 1;
  assert.deepEqual(
    rendered(),
    Array.from({ length: 20 }, (_, index) => index + 20),
  );
  model.page.value = 2;
  assert.deepEqual(rendered(), [40]);
  model.select('collected');
  assert.equal(model.pages.value, 2);
  assert.deepEqual(
    rendered(),
    Array.from({ length: 20 }, (_, index) => index + 100),
  );
  model.page.value = 1;
  assert.deepEqual(rendered(), [120]);
});
