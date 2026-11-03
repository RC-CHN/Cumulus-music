import assert from 'node:assert/strict';
import { test } from 'node:test';
import { playbackMetadata } from '../src/services/playbackMetadata.ts';

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
