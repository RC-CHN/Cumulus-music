import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  batchColumnWidths,
  batchDefaultGrowth,
  resizeBatchColumns,
  batchSelectionNeighbor,
} from '../src/services/playlistBatchTable.ts';

function close(actual: number[], expected: number[]) {
  assert.equal(actual.length, expected.length);
  actual.forEach((value, i) => {
    assert.ok(Math.abs(value - expected[i]!) < 1e-8, `${value} differs from ${expected[i]}`);
  });
}

test('official 352.Ze column bases and growth preserve fixed checkbox and heart columns', () => {
  assert.deepEqual(batchColumnWidths(464, batchDefaultGrowth), [45, 180, 115, 60, 64]);
  assert.deepEqual(batchColumnWidths(1084, batchDefaultGrowth), [45, 540, 345, 60, 94]);
  const narrow = batchColumnWidths(528, batchDefaultGrowth);
  assert.equal(narrow[0], 45);
  assert.equal(narrow[3], 60);
  assert.ok(Math.abs(narrow.reduce((a, b) => a + b, 0) - 528) < 1e-8);
  assert.ok(narrow[1]! > 180 && narrow[2]! > 115 && narrow[4]! > 64);
});
test('dragging title consumes album surplus before duration, leaving the fixed heart width alone', () => {
  const resized = resizeBatchColumns(batchDefaultGrowth, 0, 250, 1084);
  close(resized, [610, 0, 10]);
  close(batchColumnWidths(1084, resized), [45, 790, 115, 60, 74]);
  assert.deepEqual(batchDefaultGrowth, [360, 230, 30]);
  close(resizeBatchColumns(resized, 0, 10000, 1084), [620, 0, 0]);
  close(resizeBatchColumns(resized, 0, -10000, 1084), [0, 610, 10]);
});
test('album boundary redistributes only to duration and preserves total through repeated drags/resizes', () => {
  assert.deepEqual(resizeBatchColumns(batchDefaultGrowth, 1, 10000, 1084), [360, 260, 0]);
  assert.deepEqual(resizeBatchColumns(batchDefaultGrowth, 1, -10000, 1084), [360, 0, 260]);
  let state = [...batchDefaultGrowth] as [number, number, number];
  for (const width of [528, 1084, 1492, 700]) {
    for (const delta of [90, -40, 300, -500, 15, -2]) {
      state = resizeBatchColumns(state, 0, delta, width);
      assert.ok(state.every((n) => n >= 0));
      assert.ok(Math.abs(state.reduce((a, b) => a + b, 0) - 620) < 1e-8);
      assert.ok(
        Math.abs(batchColumnWidths(width, state).reduce((a, b) => a + b, 0) - width) < 1e-8,
      );
    }
  }
  assert.equal(resizeBatchColumns(state, 0, 20, 464), state);
  assert.equal(resizeBatchColumns(state, 0, Number.NaN, 800), state);
});
test('plain arrows collapse from the entire selected range, including offscreen selections', () => {
  const ids = ['1', '2', '3', '4', '5', '6'];
  assert.equal(batchSelectionNeighbor(ids, new Set(['2', '5']), -1), 0);
  assert.equal(batchSelectionNeighbor(ids, new Set(['2', '5']), 1), 5);
  assert.equal(batchSelectionNeighbor(ids, new Set(ids), -1), 0);
  assert.equal(batchSelectionNeighbor(ids, new Set(ids), 1), 5);
  assert.equal(batchSelectionNeighbor(ids, new Set(), 1), undefined);
  assert.equal(batchSelectionNeighbor(ids, new Set(['gone']), -1), undefined);
});
