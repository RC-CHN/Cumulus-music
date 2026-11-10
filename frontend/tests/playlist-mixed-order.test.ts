import assert from 'node:assert/strict';
import test from 'node:test';
import { effectScope, shallowRef } from 'vue';
import { useMixedPlaylistReorder } from '../src/composables/useMixedPlaylistReorder.ts';
import { mixedDraggedRows } from '../src/services/playlistMixedOrder.ts';
import type { AccountData, TrackPage, TrackView } from '../src/services/contracts.ts';
import type {
  PlaylistMixedOrderRow,
  PlaylistMixedReorderReceipt,
} from '../src/services/playlists.ts';

const data = <T>(value: T, epoch = '1'): AccountData<T> => ({ value, epoch });
const track = (id: string, local = false): TrackView => ({
  id,
  name: id,
  artists: [],
  album: '',
  coverUrl: null,
  durationMs: 0,
  fee: 0,
  local,
  missing: true,
});
const online = (id: string): PlaylistMixedOrderRow => ({ kind: 'online', id });
const turn = () => new Promise<void>((resolve) => setImmediate(resolve));
const receipt = (phase: PlaylistMixedReorderReceipt['phase']): PlaylistMixedReorderReceipt => ({
  playlistId: '10',
  operationId: 'operation',
  phase,
  evidence: phase === 'confirmed' ? 'changed_readback' : null,
  serviceCode: null,
});

test('partial mixed rows keep selection order and bind device references without guessing legacy IDs', () => {
  const entries = [track('004'), track('device', true), track('3')];
  const page: TrackPage = {
    revision: 'raw',
    tracks: entries,
    total: 200,
    offset: 0,
    more: true,
    playlist: {
      id: '10',
      name: 'Test',
      coverUrl: null,
      description: '',
      creator: '',
      trackCount: 200,
      owned: true,
      favorite: false,
      private: true,
    },
    playlistLocal: { revision: 'mixed', hasMembers: true, rows: { device: 'revision' } },
  };
  const shown = new Map(entries.map((item) => [item.id, item]));
  assert.deepEqual(mixedDraggedRows(page, shown, new Set(['3', 'device', '004']), '004'), [
    online('3'),
    { kind: 'local', id: 'device', revision: 'revision' },
    online('004'),
  ]);
  assert.deepEqual(mixedDraggedRows(page, shown, new Set(['3']), '004'), [online('004')]);
  assert.equal(mixedDraggedRows(page, shown, new Set(['3', 'off-page']), '3'), undefined);
  assert.equal(
    mixedDraggedRows(
      { ...page, playlistLocal: { ...page.playlistLocal!, hasUnboundLegacy: true } },
      shown,
      new Set(),
      '004',
    ),
    undefined,
  );
});

test('lost IPC receipt is reread and resolved only by the explicit read-only check', async () => {
  const scope = effectScope();
  const visit = shallowRef({ id: '10', epoch: '1' });
  let stored: PlaylistMixedReorderReceipt | null = null;
  let sends = 0,
    verifies = 0,
    refreshes = 0;
  const model = scope.run(() =>
    useMixedPlaylistReorder(
      {
        playlistMixedReorderReceipt: async () => data(stored),
        movePlaylistMixed: async () => {
          sends++;
          stored = receipt('sending');
          throw Error('lost IPC');
        },
        reconcilePlaylistMixedReorder: async (_epoch, _id, operationId) => {
          assert.equal(operationId, 'operation');
          verifies++;
          stored = receipt(verifies === 1 ? 'uncertain' : 'confirmed');
          return data(stored);
        },
      },
      () => visit.value,
      async () => {
        refreshes++;
        return true;
      },
    ),
  )!;
  try {
    await turn();
    assert.equal(model.ready.value, true);
    await model.send('revision', [online('7')], 0, '7');
    assert.equal(model.pending.value, true);
    assert.equal(refreshes, 0);
    await model.send('revision', [online('7')], 0, '7');
    assert.equal(sends, 1);
    await model.verify();
    assert.equal(model.pending.value, true);
    assert.equal(refreshes, 0);
    await model.verify();
    assert.equal(model.pending.value, false);
    assert.equal(refreshes, 1);
    assert.equal(sends, 1);
  } finally {
    scope.stop();
  }
});

test('a reply after leaving the original visit does not refresh or unlock the new view', async () => {
  const scope = effectScope();
  const visit = shallowRef({ id: '10', epoch: '1' });
  let resolve!: (reply: AccountData<PlaylistMixedReorderReceipt>) => void;
  const response = new Promise<AccountData<PlaylistMixedReorderReceipt>>((done) => {
    resolve = done;
  });
  let refreshes = 0;
  const model = scope.run(() =>
    useMixedPlaylistReorder(
      {
        playlistMixedReorderReceipt: async (_epoch, id) =>
          data(
            id === '10' ? null : { ...receipt('uncertain'), playlistId: id },
            id === '10' ? '1' : '2',
          ),
        movePlaylistMixed: async () => response,
        reconcilePlaylistMixedReorder: async () => data(receipt('confirmed')),
      },
      () => visit.value,
      async () => {
        refreshes++;
        return true;
      },
    ),
  )!;
  try {
    await turn();
    const sent = model.send('revision', [online('7')], 0, '7');
    visit.value = { id: '20', epoch: '2' };
    await turn();
    resolve(data(receipt('confirmed')));
    await sent;
    assert.equal(refreshes, 0);
    assert.equal(model.pending.value, true);
    assert.equal(model.ready.value, false);
  } finally {
    scope.stop();
  }
});
