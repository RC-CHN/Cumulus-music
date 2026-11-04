import { test } from 'node:test';
import assert from 'node:assert/strict';
import { shallowRef } from 'vue';
import { usePlaylistTagCatalogue } from '../src/composables/usePlaylistTagCatalogue.ts';
import type {
  AccountData,
  AccountView,
  CoreClient,
  PlaylistTagGroup,
} from '../src/services/contracts.ts';

test('a departed account cannot fill the catalogue cache for its replacement', async () => {
  const account = shallowRef<AccountView>({
    epoch: '1',
    status: 'authenticated',
    nickname: null,
    avatarUrl: null,
  });
  let old!: (reply: AccountData<PlaylistTagGroup[]>) => void;
  const reads: string[] = [];
  const tag = (name: string) => ({ id: 0, name, tags: [{ name, displayName: null }] });
  const core: Pick<CoreClient, 'playlistTagCatalogue'> = {
    playlistTagCatalogue(epoch) {
      reads.push(epoch);
      return epoch === '1'
        ? new Promise((resolve) => (old = resolve))
        : Promise.resolve({ epoch, value: [tag('华语')] });
    },
  };
  const model = usePlaylistTagCatalogue(
    core,
    () => account.value,
    () => assert.fail('unexpected expiry'),
  );
  const pending = model.load();
  await model.load();
  assert.deepEqual(reads, ['1']);
  account.value = { ...account.value, epoch: '2' };
  await model.load();
  old({ epoch: '1', value: [tag('旧账号结果')] });
  await pending;
  await model.load();
  assert.deepEqual(model.groups.value, [tag('华语')]);
  assert.deepEqual(reads, ['1', '2']);
});
