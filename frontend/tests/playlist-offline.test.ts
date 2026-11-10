import assert from 'node:assert/strict';
import test from 'node:test';
import {
  offlinePlaylistPlayableIds,
  offlinePlaylistSelection,
} from '../src/services/offlinePlaylist.ts';
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
    cached_at_ms: 1000,
    start_id: 'local',
    double_click: true,
  });
  assert.deepEqual(offlinePlaylistSelection('73', page), {
    kind: 'offline_playlist',
    id: '73',
    revision: 'mixed',
    cached_at_ms: 1000,
    start_id: null,
  });
  for (const track of [stale, unbound, online, { ...local }])
    assert.equal(offlinePlaylistSelection('73', page, track), undefined);
  const damaged = make('124', false, true);
  const voice = { ...make('126', false), voiceId: '126' };
  const published: TrackPage = {
    ...page,
    tracks: [...page.tracks, damaged, voice],
    playlistOffline: { cachedAtMs: 1000, downloadIds: ['123', '124', '125', '126', 'legacy'] },
  };
  assert.deepEqual([...offlinePlaylistPlayableIds(published)], ['local', '123']);
  assert.deepEqual(offlinePlaylistSelection('73', published, online, true), {
    kind: 'offline_playlist',
    id: '73',
    revision: 'mixed',
    cached_at_ms: 1000,
    start_id: '123',
    start_local: false,
    double_click: true,
  });
  for (const track of [damaged, voice, unbound, make('125', false)])
    assert.equal(offlinePlaylistSelection('73', published, track), undefined);
  assert.equal(offlinePlaylistSelection('74', page, local), undefined);
  assert.equal(
    offlinePlaylistSelection('73', { ...page, playlistOffline: undefined }, local),
    undefined,
  );
});

test('a nonempty cached playlist header is independent of loaded-page availability', () => {
  const first = {
    tracks: [],
    total: 101,
    offset: 0,
    more: true,
    playlist: { id: '73' },
    playlistLocal: { revision: 'mixed', rows: {}, hasMembers: false },
    playlistOffline: { cachedAtMs: 1000, downloadIds: [] },
  } as TrackPage;
  assert.equal(offlinePlaylistPlayableIds(first).size, 0);
  assert.deepEqual(offlinePlaylistSelection('73', first), {
    kind: 'offline_playlist',
    id: '73',
    revision: 'mixed',
    cached_at_ms: 1000,
    start_id: null,
  });
  assert.equal(offlinePlaylistSelection('73', { ...first, total: 0, more: false }), undefined);
  assert.equal(offlinePlaylistSelection('74', first), undefined);
});
