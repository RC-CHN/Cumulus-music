import assert from 'node:assert/strict';
import { test } from 'node:test';
import { lyricAnchor, lyricOffsets, lyricRange } from '../src/services/lyricWindow.ts';

test('a long expanded excerpt is one measured row and preserves offsets across resizing', () => {
  const before = lyricOffsets(
    Array.from({ length: 150 }, (_, index) => (index === 20 ? 9400 : 130)),
  );
  const top = before[20]! + 2500;
  assert.deepEqual(lyricAnchor(before, top), { index: 20, offset: 2500 });
  assert.deepEqual(lyricRange(before, top, 540), { start: 15, end: 26 });
  const after = lyricOffsets(
    Array.from({ length: 150 }, (_, index) => (index === 20 ? 9400 : 160)),
  );
  assert.deepEqual(lyricAnchor(after, after[20]! + 2500), lyricAnchor(before, top));
  assert.equal(lyricAnchor(after, after.at(-1)!), undefined);
  assert.deepEqual(lyricRange([0], 0, 540), { start: 0, end: 0 });
});
