import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useLocalBatchFavorite } from '../src/composables/useLocalBatchFavorite.ts';
import type { AccountData, PlaylistView } from '../src/services/contracts.ts';
import {
  defaultLocalQuery,
  type LocalCollectClient,
  type LocalCollectPlan,
} from '../src/services/local.ts';

const selection = {
  query: defaultLocalQuery(),
  revision: 'index-1',
  tracks: [
    { id: 'native-a', revision: 'revision-a' },
    { id: 'native-b', revision: 'revision-b' },
  ],
};
const playlist = (id: string) =>
  ({ id, name: `list ${id}`, owned: true, favorite: false }) as PlaylistView;

test('a local picker sends only an issued destination and a lost reply can only be read back', async () => {
  const calls: unknown[][] = [];
  const core: LocalCollectClient = {
    async localCollectPrepare(epoch, value) {
      calls.push(['prepare', epoch, value]);
      return { epoch, value: { receipt: 'frozen', count: 2 } };
    },
    async localCollectDestinations(epoch, receipt, sort, page) {
      calls.push(['destinations', epoch, receipt, sort, page]);
      return { epoch, value: { playlists: [playlist('81'), playlist('82')], nextPage: null } };
    },
    async localCollectExisting(epoch, receipt, target, readOnly) {
      calls.push(['select', epoch, receipt, target, readOnly]);
      if (!readOnly) throw { code: 'connection', message: 'synthetic lost reply' };
      return { epoch, value: { phase: 'confirmed', duplicate: false } };
    },
    async localCollectCancel(epoch, receipt) {
      calls.push(['cancel', epoch, receipt]);
    },
  };
  const model = useLocalBatchFavorite(core, (epoch) => epoch === '1');
  await model.open('1', selection);
  assert.deepEqual(calls, [
    ['prepare', '1', selection],
    ['destinations', '1', 'frozen', 'default', 0],
  ]);
  await model.collect('98');
  await model.collect('81', true);
  assert.equal(calls.length, 2, 'unissued target and premature reconcile cannot dispatch');
  await model.collect('81');
  assert.equal(model.pending.value, true);
  await model.collect('81');
  await model.collect('82', true);
  assert.equal(calls.length, 3, 'repeat click and alternate target cannot replay a write');
  await model.collect('81', true);
  assert.deepEqual(calls.slice(2), [
    ['select', '1', 'frozen', '81', false],
    ['select', '1', 'frozen', '81', true],
    ['cancel', '1', 'frozen'],
  ]);
  assert.equal(model.scope.value, undefined);
  assert.equal(model.notice.value, '已收藏到歌单。');
});

test('a picker prepare finishing after an account switch is cancelled without listing or writing', async () => {
  let account = '1';
  let resolve!: (value: AccountData<LocalCollectPlan>) => void;
  const held = new Promise<AccountData<LocalCollectPlan>>((fulfill) => {
    resolve = fulfill;
  });
  const cancellations: string[][] = [];
  const model = useLocalBatchFavorite(
    {
      localCollectPrepare: () => held,
      localCollectDestinations: async () => {
        throw new Error('must not list after account switch');
      },
      localCollectExisting: async () => {
        throw new Error('must not write after account switch');
      },
      localCollectCancel: async (epoch, receipt) => {
        cancellations.push([epoch, receipt]);
      },
    },
    (epoch) => epoch === account,
  );
  const opening = model.open('1', selection);
  account = '2';
  model.close();
  resolve({ epoch: '1', value: { receipt: 'old', count: 2 } });
  await opening;
  assert.deepEqual(cancellations, [['1', 'old']]);
  assert.equal(model.scope.value, undefined);
  assert.equal(model.preparing.value, false);
});
