import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  initialPlaylistShares,
  playlistBaseWidth,
  readPlaylistColumnShares,
  resizePlaylistColumns,
  writePlaylistColumnShares,
} from '../src/services/searchPlaylistColumns.ts';

const widths = (shares: readonly number[], free: number) =>
  shares.map((share) => Math.round(share * free));

test('playlist resizing transfers only surplus from each resizable neighbor on the right', () => {
  assert.equal(playlistBaseWidth, 544);
  assert.deepEqual(widths(initialPlaylistShares, 120), [80, 20, 20, 0]);
  const wider = resizePlaylistColumns(initialPlaylistShares, 0, 30, 120);
  assert.deepEqual(widths(wider, 120), [110, 0, 10, 0]);
  assert.deepEqual(widths(resizePlaylistColumns(wider, 0, 200, 120), 120), [120, 0, 0, 0]);
});

test('the last visible handle can give surplus to the play-count column', () => {
  const widerCount = resizePlaylistColumns(initialPlaylistShares, 2, -30, 120);
  assert.deepEqual(widths(widerCount, 120), [80, 20, 0, 20]);
  assert.deepEqual(widths(resizePlaylistColumns(widerCount, 2, 10, 120), 240), [160, 40, 20, 20]);
});

test('zero surplus and left and right clamps preserve the column minimums', () => {
  assert.equal(resizePlaylistColumns(initialPlaylistShares, 1, 20, 0), initialPlaylistShares);
  const narrow = resizePlaylistColumns(initialPlaylistShares, 0, -500, 120);
  assert.deepEqual(widths(narrow, 120), [0, 100, 20, 0]);
  assert.deepEqual(widths(initialPlaylistShares, 120), [80, 20, 20, 0]);
});

test('column shares reopen by layout signature and reject invalid or inaccessible storage', () => {
  const records = new Map<string, string>();
  const storage = {
    getItem: (key: string) => records.get(key) ?? null,
    setItem: (key: string, value: string) => void records.set(key, value),
  };
  const resized = resizePlaylistColumns(initialPlaylistShares, 2, -12, 120);
  assert.equal(readPlaylistColumnShares(storage), initialPlaylistShares);
  writePlaylistColumnShares(resized, storage);
  assert.deepEqual(readPlaylistColumnShares(storage), resized);
  const [key] = records.keys();
  assert.ok(key?.endsWith('num-0-45:title-4-200:trackCount-1-79:creator-1-128:playCount-0-92'));
  for (const value of ['{', '[0,0,0,0]', '[1.2,0,0,-0.2]', '["1",0,0,0]', '[1,0,0]']) {
    records.set(key, value);
    assert.equal(readPlaylistColumnShares(storage), initialPlaylistShares);
  }
  const blocked = () => {
    throw new Error('storage unavailable');
  };
  assert.equal(readPlaylistColumnShares({ getItem: blocked }), initialPlaylistShares);
  assert.doesNotThrow(() => writePlaylistColumnShares(resized, { setItem: blocked }));
});
