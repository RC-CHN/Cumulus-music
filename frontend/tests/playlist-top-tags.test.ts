import { test } from 'node:test';
import assert from 'node:assert/strict';
import { shallowRef } from 'vue';
import { usePlaylistTopTags } from '../src/composables/usePlaylistTopTags.ts';
import type {
  AccountData,
  AccountView,
  CoreClient,
  PlaylistTopTags,
} from '../src/services/contracts.ts';

test('optional top pills survive retry and a stale response cannot undo a privacy or account change', async () => {
  const account = shallowRef<AccountView>({
    epoch: 'a',
    status: 'authenticated',
    nickname: null,
    avatarUrl: null,
  });
  const reads: string[] = [];
  let resolve!: (value: AccountData<PlaylistTopTags>) => void;
  const client: Pick<CoreClient, 'playlistTopTags'> = {
    playlistTopTags(epoch) {
      reads.push(epoch);
      if (reads.length === 1) return Promise.reject({ code: 'network', message: '读取失败' });
      if (epoch === 'a') return new Promise((done) => (resolve = done));
      return Promise.resolve({
        epoch,
        value: { recommendationEnabled: true, tags: ['新账号标签'] },
      });
    },
  };
  const model = usePlaylistTopTags(
    client,
    () => account.value,
    () => assert.fail('unexpected expiry'),
  );
  await model.load();
  assert.equal(model.error.value, '读取失败');
  assert.deepEqual(model.tags.value, []);
  const retry = model.load();
  model.observePersonalService({ epoch: 'a', value: 2 });
  resolve({ epoch: 'a', value: { recommendationEnabled: true, tags: ['关闭后的旧回包'] } });
  await retry;
  await model.load();
  assert.deepEqual(model.tags.value, []);
  assert.deepEqual(reads, ['a', 'a']);
  account.value = { ...account.value, epoch: 'b' };
  await model.load();
  model.observePersonalService({ epoch: 'a', value: 2 });
  assert.deepEqual(model.tags.value, ['新账号标签']);
  assert.deepEqual(reads, ['a', 'a', 'b']);
});
