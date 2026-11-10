import assert from 'node:assert/strict';
import test from 'node:test';
import { effectScope, ref, shallowRef } from 'vue';
import { usePlaylistDuration } from '../src/composables/usePlaylistDuration.ts';
import type { Page } from '../src/composables/useMusic.ts';
import type { AccountData, AccountView, TrackPage } from '../src/services/contracts.ts';
import { sortedPlaylistRows, type PlaylistDurationPage } from '../src/services/playlistDuration.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((settle) => (resolve = settle));
  return { promise, resolve };
}

const partial = (id: string) =>
  ({
    revision: 'same-revision',
    playlist: { id },
    tracks: [{ id: 'natural-first' }],
    offset: 0,
    total: 2,
    more: true,
  }) as TrackPage;

function full(id: string): PlaylistDurationPage {
  const page = partial(id);
  page.tracks.push({ ...page.tracks[0], id: 'late-shorter' });
  page.more = false;
  return { receipt: `scope-${id}`, page, ascending: [1, 0], descending: [0, 1] };
}

test('complete duration order follows the route and cannot publish after reset or account change', async () => {
  const vue = effectScope();
  const route = shallowRef<Page>({ kind: 'playlist', id: '73' });
  const account = shallowRef<AccountView>({
    epoch: '1',
    status: 'authenticated',
    nickname: null,
    avatarUrl: null,
  });
  const shown = shallowRef<TrackPage>(partial('73'));
  const loading = ref(false);
  const reads: [string, string, ReturnType<typeof deferred<AccountData<PlaylistDurationPage>>>][] =
    [];
  const sorter = vue.run(() =>
    usePlaylistDuration(
      {
        playlistDurationPrepare: (epoch, id) => {
          const response = deferred<AccountData<PlaylistDurationPage>>();
          reads.push([epoch, id, response]);
          return response.promise;
        },
      },
      () => account.value,
      () => route.value,
      shown,
      () => loading.value,
    ),
  )!;
  try {
    sorter.cycle();
    assert.equal(sorter.order.value, 'ascending');
    assert.equal(sorter.blocked.value, true);
    assert.equal(sorter.ordinals.value, undefined);
    sorter.cycle();
    assert.equal(sorter.order.value, 'descending');
    assert.equal(reads.length, 1);
    sorter.cycle();
    reads[0][2].resolve({ epoch: '1', value: full('73') });
    await reads[0][2].promise;
    assert.equal(sorter.order.value, 'original');
    assert.equal(shown.value.more, true);
    assert.equal(sorter.scope.value, undefined);

    sorter.cycle();
    const prepared = full('73');
    reads[1][2].resolve({ epoch: '1', value: prepared });
    await reads[1][2].promise;
    assert.equal(shown.value, prepared.page);
    assert.deepEqual(sorter.ordinals.value, [1, 0]);
    assert.deepEqual(sorter.scope.value, { receipt: 'scope-73', order: 'ascending' });
    assert.deepEqual(
      sortedPlaylistRows(
        { tracks: [prepared.page.tracks[1]], positions: [1] },
        sorter.ordinals.value,
      ).positions,
      [1],
    );
    sorter.cycle();
    assert.deepEqual(sorter.ordinals.value, [0, 1]);
    assert.equal(reads.length, 2);

    route.value = { kind: 'playlist', id: '74' };
    shown.value = partial('74');
    assert.equal(sorter.order.value, 'original');
    route.value = { kind: 'playlist', id: '73' };
    shown.value = partial('73');
    assert.equal(sorter.order.value, 'descending');
    assert.equal(sorter.blocked.value, true);
    assert.deepEqual(reads[2].slice(0, 2), ['1', '73']);
    account.value = { ...account.value, epoch: '2' };
    reads[2][2].resolve({ epoch: '1', value: full('73') });
    await reads[2][2].promise;
    assert.equal(sorter.order.value, 'original');
    assert.equal(sorter.scope.value, undefined);
    assert.equal(shown.value.more, true);
  } finally {
    vue.stop();
  }
});
