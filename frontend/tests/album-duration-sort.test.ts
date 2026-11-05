import assert from 'node:assert/strict';
import { test } from 'node:test';
import { nextAlbumDurationOrder, sortAlbumDuration } from '../src/services/albumDurationSort.ts';
import type { TrackView } from '../src/services/contracts.ts';

const row = (id: string, durationMs: number) => ({ id, durationMs }) as TrackView;
const tracks = [row('11', 180_000), row('12', 90_000), row('13', 180_000), row('14', 120_000)];

test('duration goes ascending, descending, then back to server order with stable ties', () => {
  const original = { tracks, positions: null };
  const ascending = nextAlbumDurationOrder('original');
  const descending = nextAlbumDurationOrder(ascending);
  assert.deepEqual(
    sortAlbumDuration(original, ascending).tracks.map((x) => x.id),
    ['12', '14', '11', '13'],
  );
  assert.deepEqual(
    sortAlbumDuration(original, descending).tracks.map((x) => x.id),
    ['11', '13', '14', '12'],
  );
  assert.equal(sortAlbumDuration(original, nextAlbumDurationOrder(descending)), original);
});

test('filter ordering leaves the authoritative source ordinals and track objects intact', () => {
  const visible = { tracks: tracks.slice(0, 3), positions: [0, 1, 2] };
  const sorted = sortAlbumDuration(visible, 'descending');
  assert.deepEqual(sorted.positions, [0, 2, 1]);
  assert.equal(sorted.tracks[1], tracks[2]);
  assert.deepEqual(visible.positions, [0, 1, 2]);
});
