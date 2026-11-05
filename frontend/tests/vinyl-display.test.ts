import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  readVinylDisplayMode,
  storeVinylDisplayMode,
  vinylCoverLyric,
} from '../src/services/vinylDisplay.ts';

test('built-in mode starts in standard and survives reopening; unknown material values are ignored', () => {
  let stored: string | null = null;
  const storage = {
    getItem: () => stored,
    setItem: (_key: string, value: string) => (stored = value),
  };
  assert.equal(readVinylDisplayMode(storage), 'classic');
  storeVinylDisplayMode('cover', storage);
  assert.equal(readVinylDisplayMode(storage), 'cover');
  stored = 'remote-vinyl-material';
  assert.equal(readVinylDisplayMode(storage), 'classic');
  assert.equal(
    readVinylDisplayMode({
      getItem() {
        throw Error('denied');
      },
    }),
    'classic',
  );
});

test('black-vinyl line uses the initial lyric and official five millisecond lookup boundary', () => {
  const lines = [
    { time: 1, lyric: 'first' },
    { time: 2, lyric: 'second' },
    { time: 4, lyric: 'third' },
  ];
  assert.equal(vinylCoverLyric(lines, 0), 'first');
  assert.equal(vinylCoverLyric(lines, 1.994), 'first');
  assert.equal(vinylCoverLyric(lines, 1.995), 'second');
  assert.equal(vinylCoverLyric(lines, 8), 'third');
  assert.equal(vinylCoverLyric([{ lyric: 'untimed' }], 0), 'untimed');
  assert.equal(vinylCoverLyric([], 8), '');
});
