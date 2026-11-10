import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  nextAlbumDurationOrder,
  nextAlbumPopularityOrder,
  sortAlbumTracks,
} from '../src/services/albumDurationSort.ts';
import type { TrackView } from '../src/services/contracts.ts';

const row = (id: string, durationMs: number) => ({ id, durationMs }) as TrackView;
const tracks = [row('11', 180_000), row('12', 90_000), row('13', 180_000), row('14', 120_000)];

test('duration goes ascending, descending, then back to server order with stable ties', () => {
  const original = { tracks, positions: null };
  const ascending = nextAlbumDurationOrder('original');
  const descending = nextAlbumDurationOrder(ascending);
  assert.deepEqual(
    sortAlbumTracks(original, ascending).tracks.map((x) => x.id),
    ['12', '14', '11', '13'],
  );
  assert.deepEqual(
    sortAlbumTracks(original, descending).tracks.map((x) => x.id),
    ['11', '13', '14', '12'],
  );
  assert.equal(sortAlbumTracks(original, nextAlbumDurationOrder(descending)), original);
});

test('filter ordering leaves the authoritative source ordinals and track objects intact', () => {
  const visible = { tracks: tracks.slice(0, 3), positions: [0, 1, 2] };
  const sorted = sortAlbumTracks(visible, 'descending');
  assert.deepEqual(sorted.positions, [0, 2, 1]);
  assert.equal(sorted.tracks[1], tracks[2]);
  assert.deepEqual(visible.positions, [0, 1, 2]);
});

test('popularity starts descending, preserves ties and shares one active sort with duration', () => {
  const scored = tracks.map((track, index) => ({
    ...track,
    albumPopularity: [0, 96, 96, 25][index],
  }));
  const original = { tracks: scored, positions: null };
  const descending = nextAlbumPopularityOrder('ascending');
  const ascending = nextAlbumPopularityOrder(descending);
  assert.equal(descending, 'popularity_descending');
  assert.equal(ascending, 'popularity_ascending');
  assert.equal(nextAlbumPopularityOrder(ascending), 'original');
  assert.equal(nextAlbumDurationOrder(descending), 'ascending');
  assert.deepEqual(sortAlbumTracks(original, descending).positions, [1, 2, 3, 0]);
  assert.deepEqual(sortAlbumTracks(original, ascending).positions, [0, 3, 1, 2]);
  const visible = { tracks: [scored[0], scored[2], scored[3]], positions: [0, 2, 3] };
  const result = sortAlbumTracks(visible, descending);
  assert.deepEqual(result.positions, [2, 3, 0]);
  assert.equal(result.tracks[0], scored[2]);
});
