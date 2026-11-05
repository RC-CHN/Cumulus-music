import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { PlayerView } from '../src/services/player.ts';
import { playbackMetadata, playbackSource } from '../src/services/playbackMetadata.ts';

test('playback details distinguish catalog-backed credits from plain names', () => {
  const track = {
    local: false,
    artists: ['First', 'Unlinked'],
    artistRefs: [
      { id: '12', name: 'First' },
      { id: null, name: 'Unlinked' },
      { id: '/arbitrary', name: 'Untrusted reference' },
    ],
    album: 'Test album',
    albumId: '7',
  };
  assert.deepEqual(playbackMetadata(track), {
    artists: [
      { name: 'First', target: { kind: 'artist', id: '12' } },
      { name: 'Unlinked', target: null },
      { name: 'Untrusted reference', target: null },
    ],
    album: { name: 'Test album', target: { kind: 'album', id: '7' } },
  });
  assert.ok(playbackMetadata({ ...track, local: true }).artists.every((item) => !item.target));
  assert.equal(playbackMetadata({ ...track, local: true }).album.target, null);
  assert.deepEqual(playbackMetadata({ ...track, artistRefs: [] }).artists, [
    { name: 'First', target: null },
    { name: 'Unlinked', target: null },
  ]);
});

test('official voice details omit provenance for Scene FM, radio and downloaded voices', () => {
  const base = {
    track: { id: '1601', name: 'Episode', artists: [], album: '', local: false, voiceId: '601' },
    fm: false,
  } as PlayerView;
  for (const entry of [
    { ...base, resourceKind: 'voice', origin: null },
    { ...base, resourceKind: 'voice', origin: { kind: 'podcast', id: '80' } },
    { ...base, resourceKind: 'downloaded', origin: { kind: 'podcast', id: '80' } },
    { ...base, resourceKind: 'catalog', origin: { kind: 'album', id: '9' } },
  ] as PlayerView[]) {
    assert.equal(playbackSource(entry), null);
  }
  assert.equal(
    playbackSource({
      ...base,
      track: { ...base.track!, voiceId: undefined },
      resourceKind: 'voice',
    }),
    null,
  );
});

test('ordinary songs preserve named sources, downloaded navigation and unknown fallback', () => {
  const track = {
    id: '9',
    name: 'Song',
    artists: ['Artist'],
    album: 'Album',
    albumId: '8',
    local: false,
  };
  const base = { track, fm: false, resourceKind: 'catalog' } as PlayerView;
  assert.deepEqual(playbackSource({ ...base, origin: { kind: 'album', id: '8' } }), {
    name: 'Album',
    target: { kind: 'album', id: '8' },
  });
  assert.deepEqual(playbackSource({ ...base, resourceKind: 'downloaded' }), {
    name: '下载管理',
    target: { kind: 'downloads' },
  });
  assert.deepEqual(playbackSource({ ...base, vinylSimilarOrigin: true }), { name: '相似歌曲' });
  assert.deepEqual(playbackSource(base), { name: '未知' });
  assert.equal(playbackSource({ ...base, fm: true }), null);
});

test('search provenance is a non-navigable label for catalog songs only', () => {
  const track = { id: '1', name: 'Song', artists: [], album: '', local: false };
  const base = { track, fm: false, resourceKind: 'catalog' } as PlayerView;
  assert.deepEqual(playbackSource({ ...base, searchOrigin: true }), { name: '搜索' });
  assert.deepEqual(playbackSource({ ...base, searchOrigin: true, catalogFileSource: true }), {
    name: '搜索',
  });
  assert.deepEqual(
    playbackSource({ ...base, searchOrigin: true, origin: { kind: 'playlist', id: '7' } }),
    { name: '歌单', target: { kind: 'playlist', id: '7' } },
  );
  assert.equal(playbackSource({ ...base, searchOrigin: true, fm: true }), null);
  assert.equal(playbackSource({ ...base, searchOrigin: true, resourceKind: 'voice' }), null);
  assert.deepEqual(playbackSource({ ...base, searchOrigin: true, resourceKind: 'downloaded' }), {
    name: '下载管理',
    target: { kind: 'downloads' },
  });
});
