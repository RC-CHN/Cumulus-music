import { test } from 'node:test';
import assert from 'node:assert/strict';
import { shallowRef } from 'vue';
import { usePlaylistSquare } from '../src/composables/usePlaylistSquare.ts';
import type {
  AccountData,
  AccountView,
  CoreClient,
  PlaylistSquarePage,
} from '../src/services/contracts.ts';

test('playlist square preserves a failed next batch and revokes old results on privacy change', async () => {
  const account = shallowRef<AccountView>({
    epoch: 'a',
    status: 'authenticated',
    nickname: null,
    avatarUrl: null,
  });
  const calls: (string | undefined)[] = [];
  let resolve!: (reply: AccountData<PlaylistSquarePage>) => void;
  let rejected = false;
  const group = (title: string) => ({ title, items: [] });
  const reply = (title: string, continuation: string | null): AccountData<PlaylistSquarePage> => ({
    epoch: 'a',
    value: { recommendationEnabled: true, groups: [group(title)], continuation },
  });
  const core: Pick<CoreClient, 'playlistSquareRecommendations'> = {
    playlistSquareRecommendations(_epoch, cursor) {
      calls.push(cursor);
      if (!cursor) return Promise.resolve(reply('一', '1'));
      if (cursor === '1') return Promise.resolve(reply('二', '2'));
      if (cursor === '2') return Promise.resolve(reply('三', '3'));
      if (cursor === '3') {
        if (!rejected) {
          rejected = true;
          return Promise.reject({ code: 'network', message: '续页失败' });
        }
        return Promise.resolve(reply('四', '4'));
      }
      return new Promise((done) => (resolve = done));
    },
  };
  const model = usePlaylistSquare(
    core,
    () => account.value,
    () => assert.fail('no expiry'),
  );
  await model.load('first');
  await model.load('more');
  await model.load('more');
  assert.equal(model.nextBatch.value, true);
  await model.load('more');
  assert.equal(model.error.value, '续页失败');
  assert.deepEqual(
    model.groups.value.map((item) => item.title),
    ['一', '二', '三'],
  );
  await model.retry();
  assert.deepEqual(
    model.groups.value.map((item) => item.title),
    ['四'],
  );
  const pending = model.load('more');
  model.observePersonalService({ epoch: 'a', value: 2 });
  resolve(reply('五', null));
  await pending;
  assert.deepEqual(calls, [undefined, '1', '2', '3', '3', '4']);
  assert.equal(model.enabled.value, false);
  assert.deepEqual(model.groups.value, []);
  assert.equal(model.continuation.value, undefined);
});
