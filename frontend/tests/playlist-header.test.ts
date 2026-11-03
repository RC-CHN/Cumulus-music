import assert from 'node:assert/strict';
import { test } from 'node:test';
import { playlistCoverCount, playlistDate } from '../src/services/playlistHeader.ts';

test('playlist dates format the local calendar day and omit unusable timestamps', () => {
  const localDay = new Date(2024, 0, 5, 0, 30);
  assert.equal(playlistDate(localDay.getTime()), '2024-01-05');
  for (const invalid of [undefined, 0, -1, 0.5, NaN, 8_640_000_000_000_001])
    assert.equal(playlistDate(invalid), null);
});

test('playlist cover counts expose the full accessible count and omit private or invalid counts', () => {
  assert.deepEqual(playlistCoverCount(12_600, false), {
    label: '播放次数：12,600',
    text: '1.3万',
  });
  assert.deepEqual(playlistCoverCount(41, false), { label: '播放次数：41', text: '41' });
  assert.equal(playlistCoverCount(12_600, true), null);
  for (const invalid of [undefined, null, 0, -1, 1.5, Infinity, Number.MAX_SAFE_INTEGER + 1])
    assert.equal(playlistCoverCount(invalid, false), null);
});
