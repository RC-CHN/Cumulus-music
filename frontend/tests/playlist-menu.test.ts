import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  playlistMenuItems,
  playlistLink,
  playlistDownloadTracks,
} from '../src/services/playlistMenu.ts';
import type { PlaylistView, TrackView, TrackPage, CoreClient } from '../src/services/contracts.ts';
const list: PlaylistView = {
  id: '2',
  name: 'Synthetic',
  description: '',
  creator: 'Fixture',
  coverUrl: null,
  trackCount: 240,
  owned: true,
  private: false,
  favorite: false,
};
const state = { lists: [list], busy: false, playable: true, preparing: false };
test('menu visibility follows resource ownership, scene, privacy and empty state', () => {
  const keys = (p: PlaylistView) =>
    playlistMenuItems(p, 'collection', state)
      .filter((i) => !i.separator)
      .map((i) => i.key);
  assert.deepEqual(keys(list), [
    'view',
    'play',
    'share',
    'copy',
    'download',
    'edit',
    'remove',
    'up',
    'down',
  ]);
  const foreign = { ...list, id: '3', owned: false };
  assert.deepEqual(keys(foreign), ['view', 'play', 'subscribe', 'share', 'copy', 'download']);
  assert.ok(!keys({ ...list, favorite: true }).includes('remove'));
  assert.ok(!keys({ ...list, favorite: true }).includes('edit'));
  const privateList = { ...list, private: true };
  assert.equal(playlistLink(privateList), '');
  for (const p of [privateList, { ...list, trackCount: 0 }]) {
    assert.ok(
      playlistMenuItems(p, 'collection', state)
        .filter((i) => ['share', 'copy'].includes(i.key))
        .every((i) => i.disabled),
    );
  }
  assert.deepEqual(
    playlistMenuItems(list, 'detail', state).map((i) => i.key),
    ['share'],
  );
  const subscribed = playlistMenuItems(foreign, 'collection', { ...state, lists: [foreign] });
  assert.equal(subscribed.find((i) => i.key === 'remove')?.label, '取消收藏');
  assert.ok(!subscribed.some((i) => i.key === 'edit' || i.key === 'subscribe'));
});
const track = (id: string, fields = {}): TrackView => ({
  id,
  name: id,
  artists: [],
  album: '',
  coverUrl: null,
  durationMs: 0,
  fee: 0,
  missing: false,
  local: false,
  ...fields,
});
test('download all traverses revision-bound pages and omits local, missing and duplicates', async () => {
  const offsets: number[] = [];
  const client: Pick<CoreClient, 'playlistPage'> = {
    async playlistPage(id, request) {
      assert.equal(id, '2');
      assert.equal(request.limit, 100);
      assert.equal(request.revision, request.offset ? 'r1' : null);
      offsets.push(request.offset);
      return {
        epoch: '1',
        value: {
          playlist: list,
          offset: request.offset,
          total: 5,
          revision: 'r1',
          more: request.offset === 0,
          tracks: request.offset
            ? [track('3', { missing: true }), track('4')]
            : [track('1'), track('1'), track('2', { local: true })],
        },
      };
    },
  };
  assert.deepEqual(
    (
      await playlistDownloadTracks(
        client,
        '1',
        '2',
        () => true,
        () => {},
      )
    ).map((t) => t.id),
    ['1', '4'],
  );
  assert.deepEqual(offsets, [0, 3]);
});
test('download rejects changed revisions, nonadvancing pages and a switched account', async () => {
  for (const mode of ['changed', 'empty', 'account', 'cancel']) {
    let calls = 0,
      active = true;
    const client: Pick<CoreClient, 'playlistPage'> = {
      async playlistPage(_, request) {
        calls++;
        if (mode === 'cancel') active = false;
        const page: TrackPage = {
          playlist: list,
          offset: request.offset,
          revision: calls > 1 ? 'changed' : 'r1',
          tracks: mode === 'empty' ? [] : [track('1')],
          total: 10,
          more: true,
        };
        return { epoch: mode === 'account' ? '2' : '1', value: page };
      },
    };
    await assert.rejects(
      playlistDownloadTracks(
        client,
        '1',
        '2',
        () => active,
        () => {},
      ),
    );
    assert.equal(calls, mode === 'changed' ? 2 : 1);
  }
});
