import { test } from 'node:test';
import assert from 'node:assert/strict';
import { rowWindow } from '../src/services/virtualWindow.ts';

test('large lists stay bounded and cover forward, reverse and jump scrolling', () => {
  let previous;
  for (const top of [0, 1, 55, 100, 800, 40000, 20000, 100, 559440]) {
    const range = rowWindow(10000, 56, 560, top, previous);
    assert.ok(range.start <= Math.floor(top / 56));
    assert.ok(range.end >= Math.ceil((top + 560) / 56));
    assert.ok(range.end - range.start <= 33);
    previous = range;
  }
});
test('scrolling within the buffer reuses the same range, without reactive churn', () => {
  const range = rowWindow(10000, 56, 560, 10000);
  for (let top = 10000; top < 10160; top++)
    assert.equal(rowWindow(10000, 56, 560, top, range), range);
  assert.notEqual(rowWindow(10000, 56, 560, 10560, range), range);
});
test('resize, shrinking queues, empty lists and negative elastic scroll are valid', () => {
  const previous = rowWindow(2000, 62, 500, 10000);
  const resized = rowWindow(2000, 62, 1200, 10000, previous);
  assert.ok(resized.end >= Math.ceil(11200 / 62));
  assert.deepEqual(rowWindow(0, 62, 500, 0, previous), { start: 0, end: 0 });
  assert.deepEqual(rowWindow(3, 62, 500, 0, previous), { start: 0, end: 3 });
  assert.deepEqual(rowWindow(3, 62, 500, -100), { start: 0, end: 3 });
});
