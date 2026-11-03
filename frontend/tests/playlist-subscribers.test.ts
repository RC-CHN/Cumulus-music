import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ref } from 'vue';
import { usePlaylistSubscribers } from '../src/composables/usePlaylistSubscribers.ts';
import type { AccountData, PlaylistSubscribersPage } from '../src/services/contracts.ts';

test('collector pages load on demand, preserve the visible page on failure and retry that page', async () => {
  const requests: number[] = [];
  const model = usePlaylistSubscribers(
    {
      playlistSubscribers: async (epoch, id, offset) => {
        assert.equal(id, '7');
        requests.push(offset);
        if (requests.length === 2) throw new Error('unavailable');
        return {
          epoch,
          value: {
            subscribers: [],
            previousOffset: offset ? 0 : null,
            nextOffset: offset ? null : 24,
          },
        };
      },
    },
    () => 'epoch',
    () => '7',
  );
  assert.deepEqual(requests, []);
  await Promise.all([model.show(), model.show()]);
  assert.deepEqual(requests, [0]);
  await model.next();
  assert.equal(model.failed.value, true);
  assert.equal(model.value.value?.nextOffset, 24);
  await model.retry();
  assert.equal(model.failed.value, false);
  assert.equal(model.hasPrevious.value, true);
  await model.previous();
  assert.deepEqual(requests, [0, 24, 24, 0]);
});

test('leaving the tab, playlist or account discards an in-flight collector read', async () => {
  for (const change of ['tab', 'playlist', 'account']) {
    const epoch = ref('old');
    const id = ref('7');
    let resolve!: (page: AccountData<PlaylistSubscribersPage>) => void;
    const model = usePlaylistSubscribers(
      { playlistSubscribers: () => new Promise((done) => (resolve = done)) },
      () => epoch.value,
      () => id.value,
    );
    const read = model.show();
    assert.equal(model.busy.value, true);
    if (change === 'tab') model.hide();
    if (change === 'playlist') id.value = '8';
    if (change === 'account') epoch.value = 'new';
    resolve({ epoch: 'old', value: { subscribers: [], previousOffset: null, nextOffset: null } });
    await read;
    assert.equal(model.value.value, null);
    assert.equal(model.open.value, false);
    assert.equal(model.busy.value, false);
  }
});
