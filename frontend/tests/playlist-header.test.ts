import assert from 'node:assert/strict';
import { test } from 'node:test';
import { playlistDate } from '../src/services/playlistHeader.ts';

test('playlist dates format the local calendar day and omit unusable timestamps', () => {
  const localDay = new Date(2024, 0, 5, 0, 30);
  assert.equal(playlistDate(localDay.getTime()), '2024-01-05');
  for (const invalid of [undefined, 0, -1, 0.5, NaN, 8_640_000_000_000_001])
    assert.equal(playlistDate(invalid), null);
});
