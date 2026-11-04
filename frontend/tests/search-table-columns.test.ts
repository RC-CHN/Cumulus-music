import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  readSearchColumnShares,
  resizeSearchColumns,
  searchTableLayouts,
  writeSearchColumnShares,
} from '../src/services/searchTableColumns.ts';

const { baseWidth: playlistBaseWidth, shares: initialPlaylistShares } = searchTableLayouts.playlist;

const widths = (shares: readonly number[], free: number) =>
  shares.map((share) => Math.round(share * free));

test('playlist resizing transfers only surplus from each resizable neighbor on the right', () => {
  assert.equal(playlistBaseWidth, 544);
  assert.deepEqual(widths(initialPlaylistShares, 120), [80, 20, 20, 0]);
  const wider = resizeSearchColumns(initialPlaylistShares, 0, 30, 120);
  assert.deepEqual(widths(wider, 120), [110, 0, 10, 0]);
  assert.deepEqual(widths(resizeSearchColumns(wider, 0, 200, 120), 120), [120, 0, 0, 0]);
});

test('the last visible handle can give surplus to the play-count column', () => {
  const widerCount = resizeSearchColumns(initialPlaylistShares, 2, -30, 120);
  assert.deepEqual(widths(widerCount, 120), [80, 20, 0, 20]);
  assert.deepEqual(widths(resizeSearchColumns(widerCount, 2, 10, 120), 240), [160, 40, 20, 20]);
});

test('zero surplus and left and right clamps preserve the column minimums', () => {
  assert.equal(resizeSearchColumns(initialPlaylistShares, 1, 20, 0), initialPlaylistShares);
  const narrow = resizeSearchColumns(initialPlaylistShares, 0, -500, 120);
  assert.deepEqual(widths(narrow, 120), [0, 100, 20, 0]);
  assert.deepEqual(widths(initialPlaylistShares, 120), [80, 20, 20, 0]);
});

test('column shares reopen by layout signature and reject invalid or inaccessible storage', () => {
  const records = new Map<string, string>();
  const storage = {
    getItem: (key: string) => records.get(key) ?? null,
    setItem: (key: string, value: string) => void records.set(key, value),
  };
  const resized = resizeSearchColumns(initialPlaylistShares, 2, -12, 120);
  assert.equal(readSearchColumnShares('playlist', storage), initialPlaylistShares);
  writeSearchColumnShares('playlist', resized, storage);
  assert.deepEqual(readSearchColumnShares('playlist', storage), resized);
  const [key] = records.keys();
  assert.ok(key?.endsWith('num-0-45:title-4-200:trackCount-1-79:creator-1-128:playCount-0-92'));
  for (const value of ['{', '[0,0,0,0]', '[1.2,0,0,-0.2]', '["1",0,0,0]', '[1,0,0]']) {
    records.set(key, value);
    assert.equal(readSearchColumnShares('playlist', storage), initialPlaylistShares);
  }
  const blocked = () => {
    throw new Error('storage unavailable');
  };
  assert.equal(readSearchColumnShares('playlist', { getItem: blocked }), initialPlaylistShares);
  assert.doesNotThrow(() => writeSearchColumnShares('playlist', resized, { setItem: blocked }));
});

test('album preserves a fixed date column and its widths cannot leak into playlists', () => {
  const records = new Map<string, string>();
  const storage = {
    getItem: (key: string) => records.get(key) ?? null,
    setItem: (key: string, value: string) => void records.set(key, value),
  };
  const album = searchTableLayouts.album;
  assert.equal(album.baseWidth, 501);
  assert.deepEqual(
    album.columns.map((column) => column.share),
    [undefined, 0, 1, undefined],
  );
  const resized = resizeSearchColumns(album.shares, 0, 25, 120);
  assert.deepEqual(widths(resized, 120), [105, 15]);
  assert.equal(resizeSearchColumns(resized, 1, 30, 120), resized);
  writeSearchColumnShares('album', resized, storage);
  assert.deepEqual(readSearchColumnShares('album', storage), resized);
  assert.equal(readSearchColumnShares('playlist', storage), initialPlaylistShares);
});

test('voice can transfer surplus through listener count to duration and has its own preference', () => {
  const layout = searchTableLayouts.voice;
  assert.equal(layout.baseWidth, 388);
  assert.deepEqual(layout.shares, [1, 0, 0]);
  assert.notEqual(layout.storageKey, searchTableLayouts.album.storageKey);
  assert.notEqual(layout.storageKey, searchTableLayouts.playlist.storageKey);
  const count = resizeSearchColumns(layout.shares, 0, -24, 120);
  const duration = resizeSearchColumns(count, 1, -12, 120);
  assert.deepEqual(widths(duration, 120), [96, 12, 12]);
});
