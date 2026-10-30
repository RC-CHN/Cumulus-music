import { resizeTableColumns, tableColumnWidths } from '../src/services/tableColumns.ts';
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  downloadColumnWidths,
  downloadTableColumns,
  downloadSection,
  downloadSectionQuery,
  nextDownloadSort,
} from '../src/services/downloadPage.ts';
import type { DownloadQuery } from '../src/services/downloads.ts';
const base: DownloadQuery = {
  tab: 'completed',
  category: 'song',
  keyword: '',
  sort: 'completed',
  descending: true,
  offset: 100,
  limit: 100,
};
test('official download tabs distinguish song/voice/MV and reset search and page', () => {
  for (const section of ['song', 'voice', 'mv', 'active'] as const) {
    const query = { ...base, keyword: 'old', ...downloadSectionQuery(section) };
    assert.equal(downloadSection(query), section);
    assert.equal(query.keyword, '');
    assert.equal(query.offset, 0);
    assert.equal(query.category, section === 'active' ? 'all' : section);
  }
});
test('download header sort cycles title, artist and binary columns from recovered configs', () => {
  let q = { ...base };
  const order: string[] = [];
  for (let i = 0; i < 5; i++) {
    q = { ...q, ...nextDownloadSort(q, 'name') };
    order.push(`${q.sort}:${q.descending}`);
  }
  assert.deepEqual(order, ['name:false', 'name:true', 'artist:false', 'artist:true', 'name:false']);
  assert.deepEqual(
    nextDownloadSort({ ...base, category: 'voice', sort: 'artist', descending: true }, 'name'),
    { sort: 'none', descending: false },
  );
  assert.deepEqual(nextDownloadSort(base, 'album'), { sort: 'album', descending: false });
  assert.deepEqual(nextDownloadSort({ ...base, sort: 'album', descending: false }, 'album'), {
    sort: 'album',
    descending: true,
  });
});
test('download tables retain official fixed bases plus their growth proportions', () => {
  assert.deepEqual(downloadColumnWidths(490, 'song'), [45, 180, 115, 90, 60]);
  assert.deepEqual(downloadColumnWidths(1155, 'song'), [45, 540, 345, 135, 90]);
  assert.deepEqual(downloadColumnWidths(810, 'voice'), [45, 540, 135, 90]);
  assert.deepEqual(downloadColumnWidths(685, 'active'), [45, 360, 280]);
  for (const section of ['song', 'voice', 'active'] as const)
    for (const width of [528, 836, 1492]) {
      const values = downloadColumnWidths(width, section);
      assert.equal(values[0], 45);
      assert.ok(Math.abs(values.reduce((a, b) => a + b, 0) - width) < 1e-8);
    }
});

test('voice sorting clears the active column after five title states or three other states', () => {
  let query: DownloadQuery = { ...base, category: 'voice' };
  const states: string[] = [];
  for (let i = 0; i < 6; i++) {
    query = { ...query, ...nextDownloadSort(query, 'name') };
    states.push(`${query.sort}:${query.descending}`);
  }
  assert.deepEqual(states, [
    'name:false',
    'name:true',
    'artist:false',
    'artist:true',
    'none:false',
    'name:false',
  ]);
  for (const sort of ['completed', 'size'] as const) {
    query = { ...base, category: 'voice', sort: 'none', descending: false };
    const actual = [];
    for (let i = 0; i < 4; i++) {
      query = { ...query, ...nextDownloadSort(query, sort) };
      actual.push(`${query.sort}:${query.descending}`);
    }
    assert.deepEqual(actual, [`${sort}:false`, `${sort}:true`, 'none:false', `${sort}:false`]);
  }
  assert.deepEqual(nextDownloadSort({ ...base, category: 'voice' }, 'completed'), {
    sort: 'none',
    descending: false,
  });
});
test('download resizers preserve fixed index and consume following columns in official order', () => {
  const columns = downloadTableColumns('song'),
    growth = columns.map((c) => c.grow);
  assert.deepEqual(
    tableColumnWidths(1155, columns, resizeTableColumns(columns, growth, 1, 250, 1155)),
    [45, 790, 115, 115, 90],
  );
  assert.deepEqual(
    tableColumnWidths(1155, columns, resizeTableColumns(columns, growth, 2, -10000, 1155)),
    [45, 540, 115, 365, 90],
  );
  for (const section of ['song', 'voice', 'active'] as const) {
    const definition = downloadTableColumns(section);
    let weights = definition.map((c) => c.grow);
    for (const width of [528, 836, 1492])
      for (const delta of [-10000, 25, 10000, -40]) {
        weights = resizeTableColumns(definition, weights, 1, delta, width);
        const sizes = tableColumnWidths(width, definition, weights);
        assert.equal(sizes[0], 45);
        assert.ok(sizes.every((v, i) => v >= definition[i]!.width));
        assert.ok(Math.abs(sizes.reduce((a, b) => a + b, 0) - width) < 1e-8);
      }
  }
});
