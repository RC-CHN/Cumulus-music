import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readPlaylistBatch, toggleBatchSelection } from '../src/services/playlistBatch.ts';
import type { CoreClient, TrackView } from '../src/services/contracts.ts';

const track = (id: number): TrackView => ({
  id: String(id),
  name: `Synthetic ${id}`,
  artists: [],
  album: '',
  coverUrl: null,
  durationMs: 1000,
  fee: 0,
  missing: id === 2,
  local: id === 3,
});
test('batch reads the whole snapshot in order including unavailable rows', async () => {
  const offsets: number[] = [],
    progress: number[] = [];
  const core: Pick<CoreClient, 'playlistPage'> = {
    async playlistPage(id, query) {
      assert.equal(id, '7');
      assert.equal(query.revision, query.offset ? 'snapshot' : null);
      offsets.push(query.offset);
      return {
        epoch: '1',
        value: {
          playlist: null,
          offset: query.offset,
          tracks: Array.from({ length: Math.min(100, 601 - query.offset) }, (_, i) =>
            track(query.offset + i + 1),
          ),
          revision: 'snapshot',
          total: 601,
          more: query.offset + 100 < 601,
        },
      };
    },
  };
  const result = await readPlaylistBatch(
    core,
    '1',
    '7',
    () => true,
    (count) => progress.push(count),
  );
  assert.deepEqual(offsets, [0, 100, 200, 300, 400, 500, 600]);
  assert.deepEqual(progress, [100, 200, 300, 400, 500, 600, 601]);
  assert.equal(result.tracks.length, 601);
  assert.equal(result.tracks[600]!.id, '601');
  assert.ok(result.tracks[1]!.missing && result.tracks[2]!.local);
  assert.equal(result.offset, 0);
});
test('batch rejects stale, incomplete, nonadvancing or mismatched snapshots before display', async () => {
  for (const mode of ['account', 'cancel', 'revision', 'missing', 'offset', 'empty', 'large']) {
    let calls = 0,
      active = true;
    const core: Pick<CoreClient, 'playlistPage'> = {
      async playlistPage(_, query) {
        calls++;
        if (mode === 'cancel') active = false;
        return {
          epoch: mode === 'account' ? '2' : '1',
          value: {
            playlist: null,
            offset: query.offset + (mode === 'offset' ? 1 : 0),
            tracks: mode === 'empty' ? [] : [track(calls)],
            total: mode === 'large' ? 50001 : 2,
            revision: mode === 'missing' ? null : calls > 1 ? 'changed' : 'snapshot',
            more: true,
          },
        };
      },
    };
    await assert.rejects(
      readPlaylistBatch(
        core,
        '1',
        '7',
        () => active,
        () => {},
      ),
    );
    assert.equal(calls, mode === 'revision' ? 2 : 1, mode);
  }
});
test('whole-row toggles preserve other selections; optional Shift ranges add inclusively', () => {
  const ids = ['1', '2', '3', '4', '5'];
  const first = toggleBatchSelection(ids, new Set(), 1, -1, false);
  const next = toggleBatchSelection(ids, first, 4, 1, false);
  assert.deepEqual([...next], ['2', '5']);
  assert.deepEqual([...first], ['2']);
  const range = toggleBatchSelection(ids, next, 3, 1, true);
  assert.deepEqual([...range].sort(), ['2', '3', '4', '5']);
  assert.deepEqual([...toggleBatchSelection(ids, range, 2, 1, false)].sort(), ['2', '4', '5']);
  assert.equal(toggleBatchSelection(ids, range, 30, 1, false), range);
});
