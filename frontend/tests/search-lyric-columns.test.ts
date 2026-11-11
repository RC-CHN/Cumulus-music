import test from 'node:test';
import assert from 'node:assert/strict';
import {
  lyricColumnGrowth,
  lyricColumnShares,
  searchLyricColumns,
} from '../src/services/searchLyricColumns.ts';
import { searchTableLayouts, writeSearchColumnShares } from '../src/services/searchTableColumns.ts';
import {
  minimumTableWidth,
  resizeTableColumns,
  tableColumnWidths,
} from '../src/services/tableColumns.ts';

const width = 772; // Base 464 plus the exact 308 official grow units.
const pixels = (growth: number[]) =>
  tableColumnWidths(width, searchLyricColumns, growth).map(Math.round);
const resize = (growth: number[], column: number, delta: number, basis = width) =>
  resizeTableColumns(searchLyricColumns, growth, column, delta, basis);

test('lyric title consumes album surplus before duration and album can only draw from duration', () => {
  const growth = lyricColumnGrowth({ getItem: () => null });
  assert.equal(minimumTableWidth(searchLyricColumns), 464);
  assert.deepEqual(pixels(growth), [45, 365, 230, 60, 72]);
  assert.deepEqual(pixels(resize(growth, 1, 120)), [45, 485, 115, 60, 67]);
  assert.deepEqual(pixels(resize(growth, 1, -999)), [45, 180, 415, 60, 72]);
  assert.deepEqual(pixels(resize(growth, 2, 999)), [45, 365, 238, 60, 64]);
  assert.equal(resize(growth, 4, 10), growth);
  assert.equal(resize(growth, 3, 10), growth);
  assert.equal(resize(growth, 1, 10, 450), growth);
});

test('lyric column storage contains only ratios with a layout key separate from other search tables', () => {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
  const growth = lyricColumnGrowth(storage);
  const changed = resize(growth, 1, 120);
  const shares = lyricColumnShares(changed);
  assert.ok(shares && shares.length === 3);
  writeSearchColumnShares('lyrics', shares, storage);
  assert.deepEqual([...values.keys()], [searchTableLayouts.lyrics.storageKey]);
  assert.notEqual(searchTableLayouts.lyrics.storageKey, searchTableLayouts.playlist.storageKey);
  assert.notEqual(searchTableLayouts.lyrics.storageKey, searchTableLayouts.album.storageKey);
  assert.deepEqual(pixels(lyricColumnGrowth(storage)), pixels(changed));
  assert.deepEqual(lyricColumnShares([0, 185, 115, 0, 8]), lyricColumnShares(growth));
  assert.equal(lyricColumnShares([0, 185, 115, 1, 8]), undefined);
  assert.equal(lyricColumnShares([0, 185, Infinity, 0, 8]), undefined);
  storage.setItem(searchTableLayouts.lyrics.storageKey, '[2,-1,0]');
  assert.deepEqual(lyricColumnGrowth(storage), growth);
});
