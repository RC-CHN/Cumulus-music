import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  searchCardSelection,
  searchPodcastCardSelection,
  searchPodcastRowSelection,
  searchAlbumCredits,
  searchCardIsCurrent,
  searchCardCanPlay,
  searchMvPlaybackTarget,
  searchPlaylistCreator,
  searchVideoCoverStats,
  searchVideoCredits,
  searchCurrentSongId,
  searchCurrentVoiceId,
  searchVoiceSelection,
  searchVoiceProgram,
  searchResourceShare,
} from '../src/services/searchCards.ts';
import type { CoreClient, TrackView } from '../src/services/contracts.ts';
import type { SearchItem, SearchSection } from '../src/services/search.ts';
const artist = { kind: 'artist', artist: { id: '12' } } as Extract<SearchItem, { kind: 'artist' }>;
const album = { kind: 'album', album: { id: '12' } } as Extract<SearchItem, { kind: 'album' }>;
const voice = {
  kind: 'voice',
  voice: { id: '21', radioId: '7', unavailable: false },
} as Extract<SearchItem, { kind: 'voice' }>;
const podcast = {
  kind: 'podcast',
  podcast: { id: '7', unavailable: false },
} as Extract<SearchItem, { kind: 'podcast' }>;

test('ordinary search links use the observed public program even without a playable radio', () => {
  const row = {
    kind: 'voice',
    voice: {
      ...voice.voice,
      name: '合成单集',
      creator: '合成主播',
      radioId: null,
      unavailable: true,
    },
  } as const;
  const section: SearchSection = {
    id: 'search_block_voice:5',
    title: '声音',
    category: 'voices',
    items: [row, podcast],
  };
  const program = { id: '21', name: '合成单集', creator: '合成主播' };
  assert.deepEqual(searchVoiceProgram('all', section, row), program);
  assert.deepEqual(searchVoiceProgram('voices', { ...section, id: '声音' }, row), program);
  assert.equal(searchVoiceProgram('all', section, { ...row }), undefined);
  assert.equal(searchVoiceProgram('all', section, podcast), undefined);
  assert.equal(searchVoiceProgram('podcasts', section, row), undefined);
  for (const id of ['search_block_best_match:0', 'search_block_hot_result_core_resources:1']) {
    assert.equal(searchVoiceProgram('all', { ...section, id }, row), undefined);
  }
  const malformed = { ...row, voice: { ...row.voice, id: '21&uid=7' } };
  assert.equal(searchVoiceProgram('all', { ...section, items: [malformed] }, malformed), undefined);
});
test('MV sharing requires an observed ordinary MV and uses its native identity', () => {
  const row = {
    kind: 'mv',
    mv: {
      id: 'display',
      kind: 'mv',
      mvId: '601',
      title: '合成 MV',
      artists: [{ name: '合成歌手' }],
    },
  } as Extract<SearchItem, { kind: 'mv' }>;
  const section: SearchSection = {
    id: 'search_block_video:7',
    title: 'MV',
    category: 'mvs',
    items: [row],
  };
  const share = {
    kind: 'mv',
    link: 'https://music.163.com/mv?id=601',
    resource: { id: '601', name: '合成 MV', creator: '合成歌手' },
  };
  assert.deepEqual(searchResourceShare('all', section, row), share);
  assert.deepEqual(searchResourceShare('mvs', { ...section, id: 'MV' }, row), share);
  assert.equal(searchResourceShare('all', section, { ...row }), undefined);
  assert.equal(
    searchResourceShare('all', { ...section, id: 'search_block_best_match:0' }, row),
    undefined,
  );
  assert.equal(searchResourceShare('albums', section, row), undefined);
  for (const candidate of [
    { ...row, mv: { ...row.mv, kind: 'video' as const } },
    { ...row, mv: { ...row.mv, mvId: '601&userid=7' } },
    { ...row, mv: { ...row.mv, mvId: null } },
  ]) {
    assert.equal(
      searchResourceShare('mvs', { ...section, items: [candidate] }, candidate),
      undefined,
    );
  }
});

test('artist covers select the native resolver and albums keep their own snapshots', async () => {
  const calls: unknown[] = [];
  const core = {
    catalogPage: async (...args: unknown[]) => {
      calls.push(args);
      return {
        epoch: '1',
        value: {
          query: { kind: 'album', id: '12' },
          revision: 'fresh',
          page: {
            kind: 'album',
            tracks: [
              { id: '1', missing: true },
              { id: '2', missing: false },
            ],
          },
        },
      };
    },
  } as unknown as CoreClient;
  assert.deepEqual(await searchCardSelection(core, '1', artist, 'lossless'), {
    kind: 'search_artist_card',
    id: '12',
    quality: 'lossless',
  });
  assert.deepEqual(calls, []);
  assert.deepEqual(await searchCardSelection(core, '1', album, 'lossless'), {
    kind: 'catalog',
    pages: [{ query: { kind: 'album', id: '12' }, revision: 'fresh' }],
    index: 1,
    quality: 'lossless',
  });
  assert.equal((calls[0] as unknown[])[2], true);
  assert.equal(calls.length, 1);
  assert.equal(searchCardIsCurrent(artist, { kind: 'album', id: '12' }), false);
  assert.equal(searchCardIsCurrent(artist, { kind: 'artist', id: '12' }), true);
  assert.equal(searchCardIsCurrent(voice, null, '21'), true);
});
test('missing catalog data cannot become an album playback selection', async () => {
  const core = {
    catalogPage: async () => ({ epoch: '1', value: null }),
  } as unknown as CoreClient;
  await assert.rejects(searchCardSelection(core, '1', album, 'standard'), { code: 'unavailable' });
});

test('comprehensive podcast cards require their exact native playback receipt', () => {
  assert.equal(searchPodcastCardSelection(podcast), undefined);
  const available = { ...podcast, playbackReceipt: 'native-page' };
  assert.deepEqual(searchPodcastCardSelection(available), {
    kind: 'search_podcast_card',
    id: '7',
    receipt: 'native-page',
  });
  assert.equal(
    searchPodcastCardSelection({
      ...available,
      podcast: { ...podcast.podcast, unavailable: true },
    }),
    undefined,
  );
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

test('MV credits keep unidentified artists as text without inventing identities', () => {
  const item: SearchItem = {
    kind: 'mv',
    mv: {
      id: '606',
      kind: 'mv',
      title: 'Synthetic MV',
      description: '',
      artists: [
        { id: '12', name: '主唱' },
        { id: null, name: ' 合作艺人 ' },
        { id: '0', name: '乐团' },
        { id: '606', name: ' ' },
      ],
      coverUrl: null,
      durationMs: 240000,
      playCount: 0,
      mvId: '606',
    },
  };
  assert.deepEqual(searchVideoCredits(item), [
    { id: '12', name: '主唱' },
    { id: null, name: '合作艺人' },
    { id: null, name: '乐团' },
  ]);
  assert.deepEqual(searchVideoCredits({ ...item, mv: { ...item.mv, mvId: null } }), []);
});

test('search playlist credits navigate only to a native-verified creator identity', () => {
  const item: SearchItem = {
    kind: 'playlist',
    playlist: {
      id: '777',
      name: 'Synthetic playlist',
      coverUrl: null,
      description: '',
      creator: ' Synthetic creator ',
      creatorId: '9007199254740993',
      trackCount: 3,
      owned: false,
      favorite: false,
      private: false,
    },
  };
  assert.deepEqual(searchPlaylistCreator(item), {
    id: '9007199254740993',
    name: 'Synthetic creator',
  });
  for (const id of [null, undefined, '0', 'name-only', 'https://invalid.example/123'])
    assert.equal(
      searchPlaylistCreator({ ...item, playlist: { ...item.playlist, creatorId: id } }),
      null,
    );
  assert.equal(
    searchPlaylistCreator({ ...item, playlist: { ...item.playlist, creator: ' ' } }),
    null,
  );
  assert.equal(searchPlaylistCreator(podcast), null);
});

test('album search credits put the primary artist first and preserve names without navigable IDs', () => {
  const lead = { id: '9007199254740993', name: ' Primary credit ' };
  const item: SearchItem = {
    kind: 'album',
    album: {
      id: '777',
      name: 'Synthetic album',
      artists: [{ id: '0', name: 'Text only' }, lead, { id: '8', name: 'Guest' }],
      primaryArtist: lead,
      aliases: [],
      translations: [],
      description: '',
      coverUrl: null,
      trackCount: 2,
      publishedAt: null,
      subscribed: null,
      explicit: false,
    },
  };
  assert.deepEqual(searchAlbumCredits(item), [
    { id: lead.id, name: 'Primary credit' },
    { id: null, name: 'Text only' },
    { id: '8', name: 'Guest' },
  ]);
  assert.deepEqual(searchAlbumCredits({ ...item, album: { ...item.album, artists: [] } }), [
    { id: lead.id, name: 'Primary credit' },
  ]);
  assert.deepEqual(searchAlbumCredits(podcast), []);
});

test('classified podcasts carry a row receipt and explicit gesture, never a card or raw ID', () => {
  const classified = { ...podcast, table: { subscriptionEligible: false }, playbackReceipt: 'row' };
  assert.equal(searchPodcastCardSelection(classified), undefined);
  assert.equal(
    searchPodcastRowSelection({ ...podcast, playbackReceipt: 'card' }, 'index'),
    undefined,
  );
  assert.equal(
    searchPodcastRowSelection({ ...classified, playbackReceipt: undefined }, 'index'),
    undefined,
  );
  for (const gesture of ['index', 'double_click', 'menu'] as const) {
    assert.deepEqual(searchPodcastRowSelection(classified, gesture), {
      kind: 'search_podcast_row',
      id: '7',
      receipt: 'row',
      gesture,
    });
  }
});
