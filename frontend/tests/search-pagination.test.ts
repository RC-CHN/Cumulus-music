import assert from 'node:assert/strict';
import test from 'node:test';
import { advanceSearchPage } from '../src/services/searchPagination.ts';

test('short result pages follow the consumed offsets in both directions', () => {
  const second = advanceSearchPage(undefined, 18)!;
  const third = advanceSearchPage(second, 29)!;
  assert.equal(second.number, 2);
  assert.equal(third.number, 3);
  assert.equal(third.previous?.offset, 18);
  assert.equal(third.previous?.previous, undefined);
  // Return from another route and page forward again without guessing 30.
  assert.equal(advanceSearchPage(third.previous, 29)?.previous, second);
});

test('search pagination rejects an unchanged, backward or out-of-range service offset', () => {
  const second = advanceSearchPage(undefined, 18);
  for (const invalid of [0, 17, 18, 18.5, 10001, Infinity, NaN])
    assert.equal(advanceSearchPage(second, invalid), undefined);
});
