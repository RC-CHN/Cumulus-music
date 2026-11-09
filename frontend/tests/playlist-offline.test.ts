import assert from 'node:assert/strict';
import test from 'node:test';
import { offlinePlaylistSelection } from '../src/services/offlinePlaylist.ts';
import type { TrackPage, TrackView } from '../src/services/contracts.ts';

test('an offline playlist click binds an existing device member, never cached catalog metadata', () => {
  const make = (id: string, local: boolean, missing = false): TrackView => ({
    id,
    name: id,
    local,
    missing,
    artists: [],
    album: '',
    coverUrl: null,
    durationMs: 1000,
    fee: 0,
  });
  const local = make('local', true);
  const stale = make('stale', true, true);
  const unbound = make('legacy', true);
  const online = make('123', false);
  const page: TrackPage = {
    revision: 'raw',
    tracks: [local, stale, unbound, online],
    total: 4,
    offset: 0,
    more: false,
    playlist: {
      id: '73',
      name: 'cache',
      coverUrl: null,
      description: '',
      creator: '',
      trackCount: 4,
      owned: true,
      private: true,
      favorite: false,
    },
    playlistLocal: {
      revision: 'mixed',
      hasMembers: true,
      rows: { local: 'file-1', stale: 'file-2' },
    },
    playlistOffline: { cachedAtMs: 1000 },
  };
  assert.deepEqual(offlinePlaylistSelection('73', page, local, true), {
    kind: 'offline_playlist',
    id: '73',
    revision: 'mixed',
    start_id: 'local',
    double_click: true,
  });
  assert.deepEqual(offlinePlaylistSelection('73', page), {
    kind: 'offline_playlist',
    id: '73',
    revision: 'mixed',
    start_id: null,
  });
  for (const track of [stale, unbound, online, { ...local }])
    assert.equal(offlinePlaylistSelection('73', page, track), undefined);
  assert.equal(offlinePlaylistSelection('74', page, local), undefined);
  assert.equal(
    offlinePlaylistSelection('73', { ...page, playlistOffline: undefined }, local),
    undefined,
  );
});
