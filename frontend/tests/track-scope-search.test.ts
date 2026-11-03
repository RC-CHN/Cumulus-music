import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { TrackView } from '../src/services/contracts.ts';
import { filterTrackScope } from '../src/services/trackScopeSearch.ts';

function song(id: string, name: string, artists: string[] = [], album = ''): TrackView {
  return {
    id,
    name,
    artists,
    album,
    coverUrl: null,
    durationMs: 0,
    fee: 0,
    missing: false,
    local: false,
  };
}

test('playlist keyword matches literal visible metadata and keeps source row positions', () => {
  const tracks = [
    song('1', 'Without match'),
    song('7', 'LIVE [B]'),
    song('2', 'artist target', ['guest [b]']),
    { ...song('3', '[b] unavailable'), missing: true },
    song('7', 'album target', [], '录音 [b]'),
  ];
  const filtered = filterTrackScope(tracks, ' [b] ');
  assert.deepEqual(filtered.positions, [1, 2, 4]);
  assert.deepEqual(filtered.tracks, [tracks[1], tracks[2], tracks[4]]);
  assert.deepEqual(filterTrackScope(tracks, '.*').positions, []);
  assert.equal(filterTrackScope(tracks, ' ').tracks, tracks);
});
