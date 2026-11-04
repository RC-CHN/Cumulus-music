import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  centeredRowTop,
  rowBoundary,
  rowExtent,
  rowTop,
  rowWindow,
} from '../src/services/virtualWindow.ts';

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

test('a centered jump brings an unmounted playing row into the bounded window', () => {
  const count = 1000;
  const height = 56;
  const viewport = 280;
  const initial = rowWindow(count, height, viewport, 0);
  assert.ok(initial.end < 700);
  const top = centeredRowTop(700, count, height, viewport);
  assert.equal(top, 700 * height + height / 2 - viewport / 2);
  const jumped = rowWindow(count, height, viewport, top!, initial);
  assert.ok(jumped.start <= 700 && jumped.end > 700);
  assert.ok(jumped.end - jumped.start <= 22);
  assert.equal(centeredRowTop(0, count, height, viewport), 0);
  assert.equal(centeredRowTop(999, count, height, viewport), count * height - viewport);
  assert.equal(centeredRowTop(1000, count, height, viewport), null);
});

test('one 74px separator row preserves window boundaries, centering and pointer drop slots', () => {
  const expansion = { index: 50, extra: 10 };
  const count = 100;
  const height = 64;
  const top = 50 * height;
  assert.equal(rowTop(50, height, expansion), top);
  assert.equal(rowExtent(50, height, expansion), 74);
  assert.equal(rowTop(51, height, expansion), top + 74);
  assert.equal(rowTop(100, height, expansion), 6410);
  assert.equal(rowBoundary(top + 36, count, height, expansion), 50);
  assert.equal(rowBoundary(top + 37, count, height, expansion), 51);
  assert.equal(rowBoundary(top + 74 + 31, count, height, expansion), 51);
  assert.equal(rowBoundary(top + 74 + 32, count, height, expansion), 52);
  assert.deepEqual(rowWindow(count, height, 64, top + 73, undefined, 0, expansion), {
    start: 50,
    end: 52,
  });
  assert.deepEqual(rowWindow(count, height, 64, top + 74, undefined, 0, expansion), {
    start: 51,
    end: 52,
  });
  assert.equal(centeredRowTop(50, count, height, 200, expansion), top + 37 - 100);
  assert.equal(centeredRowTop(99, count, height, 200, expansion), 6210);
});
