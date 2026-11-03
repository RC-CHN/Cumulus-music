import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  searchCardSelection,
  searchCardIsCurrent,
  searchCardCanPlay,
  searchMvPlaybackTarget,
  searchVideoCoverStats,
  searchCurrentSongId,
  searchCurrentVoiceId,
  searchVoiceSelection,
} from '../src/services/searchCards.ts';
import type { CoreClient, TrackView } from '../src/services/contracts.ts';
import type { SearchItem } from '../src/services/search.ts';
const artist = { kind: 'artist', artist: { id: '12' } } as Extract<SearchItem, { kind: 'artist' }>;
const voice = {
  kind: 'voice',
  voice: { id: '21', radioId: '7', unavailable: false },
} as Extract<SearchItem, { kind: 'voice' }>;
const podcast = {
  kind: 'podcast',
  podcast: { id: '7', unavailable: false },
} as Extract<SearchItem, { kind: 'podcast' }>;
test('cards select authoritative catalog and radio revisions, not cached search titles', async () => {
  const calls: unknown[] = [];
  const core = {
    catalogPage: async (...args: unknown[]) => {
      calls.push(args);
      return {
        epoch: '1',
        value: {
          query: { kind: 'artist', id: '12' },
          revision: 'fresh',
          page: {
            kind: 'artist',
            tracks: [
              { id: '1', missing: true },
              { id: '2', missing: false },
            ],
          },
        },
      };
    },
    podcastPage: async (...args: unknown[]) => {
      calls.push(args);
      return { epoch: '1', revision: 'radio-fresh', episodes: [{ id: '21', unavailable: false }] };
    },
  } as unknown as CoreClient;
  assert.deepEqual(await searchCardSelection(core, '1', artist, 'lossless'), {
    kind: 'catalog',
    pages: [{ query: { kind: 'artist', id: '12' }, revision: 'fresh' }],
    index: 1,
    quality: 'lossless',
  });
  assert.deepEqual(await searchCardSelection(core, '1', podcast, 'standard'), {
    kind: 'podcast',
    scope: { kind: 'radio', id: '7', ascending: true },
    revision: 'radio-fresh',
    start_id: '21',
  });
  assert.equal((calls[0] as unknown[])[2], true);
  assert.equal((calls[1] as unknown[])[2], true);
  assert.equal(searchCardIsCurrent(artist, { kind: 'album', id: '12' }), false);
  assert.equal(searchCardIsCurrent(artist, { kind: 'artist', id: '12' }), true);
  assert.equal(searchCardIsCurrent(voice, null, '21'), true);
});
test('missing catalog data and stale radio snapshots cannot become playback selections', async () => {
  const core = {
    catalogPage: async () => ({ epoch: '1', value: null }),
    podcastPage: async () => ({ epoch: 'old', revision: 'stale' }),
  } as unknown as CoreClient;
  await assert.rejects(searchCardSelection(core, '1', artist, 'standard'), { code: 'unavailable' });
  await assert.rejects(searchCardSelection(core, '1', podcast, 'standard'), { code: 'stale' });
});

test('search episodes select only their own ID with or without a parent podcast', () => {
  const standalone = { ...voice, voice: { ...voice.voice, radioId: null } };
  for (const item of [voice, standalone]) {
    assert.deepEqual(searchVoiceSelection(item), { kind: 'search_result_voice', id: '21' });
    assert.equal(
      searchVoiceSelection({ ...item, voice: { ...item.voice, unavailable: true } }),
      undefined,
    );
  }
});

test('only a current catalog song supplies a search song highlight or toggle ID', () => {
  const track: TrackView = {
    id: '1',
    name: 'Shared media ID',
    artists: [],
    album: '',
    coverUrl: null,
    durationMs: 1000,
    fee: 0,
    missing: false,
    local: false,
  };
  assert.equal(searchCurrentSongId({ resourceKind: 'catalog', track }), '1');
  for (const resourceKind of ['voice', 'cloud', 'local', 'downloaded', null, undefined] as const)
    assert.equal(searchCurrentSongId({ resourceKind, track }), undefined);
  assert.equal(searchCurrentSongId({ resourceKind: 'catalog', track: null }), undefined);
  const episodeTrack = { ...track, voiceId: voice.voice.id };
  assert.equal(
    searchCardIsCurrent(
      voice,
      null,
      searchCurrentVoiceId({ resourceKind: 'voice', track: episodeTrack }),
    ),
    true,
  );
  assert.equal(
    searchCardIsCurrent(
      voice,
      null,
      searchCurrentVoiceId({ resourceKind: 'downloaded', track: episodeTrack }),
    ),
    false,
  );
});

test('search videos open the native MV target only, separate from audio selection', () => {
  const mv: SearchItem = {
    kind: 'mv',
    mv: {
      id: 'presentation-id',
      kind: 'mv',
      title: 'Synthetic MV',
      description: '',
      artists: [],
      coverUrl: null,
      durationMs: 30000,
      playCount: 0,
      mvId: '9007199254740993',
    },
  };
  assert.equal(searchMvPlaybackTarget(mv), '9007199254740993');
  assert.equal(searchCardCanPlay(mv), false);
  assert.equal(searchMvPlaybackTarget({ ...mv, mv: { ...mv.mv, mvId: null } }), null);
  assert.equal(searchMvPlaybackTarget({ ...mv, mv: { ...mv.mv, kind: 'video' } }), null);
  assert.equal(searchMvPlaybackTarget(voice), null);
});

test('video cover stats show known duration and a compact count with exact accessible context', () => {
  const mv: SearchItem = {
    kind: 'mv',
    mv: {
      id: '606',
      kind: 'mv',
      title: 'Synthetic MV',
      description: '',
      artists: [],
      coverUrl: null,
      durationMs: 240999,
      playCount: 12345,
      mvId: '606',
    },
  };
  assert.deepEqual(searchVideoCoverStats(mv), {
    duration: '04:00',
    count: '1.2万',
    description: '播放次数：12,345，时长：04:00',
  });
  assert.deepEqual(
    searchVideoCoverStats({ ...mv, mv: { ...mv.mv, playCount: 0, durationMs: 79000 } }),
    {
      duration: '01:19',
      count: null,
      description: '时长：01:19',
    },
  );
  assert.equal(
    searchVideoCoverStats({
      ...mv,
      mv: { ...mv.mv, durationMs: Number.NaN, playCount: Number.MAX_SAFE_INTEGER + 1 },
    }),
    null,
  );
  assert.equal(searchVideoCoverStats(voice), null);
});
