import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  selectQueueRows,
  reorderQueueRows,
  type QueueSelection,
} from '../src/services/queueSelection.ts';

const ids = ['a', 'b', 'c', 'd', 'e', 'f'];
const selection = (...selected: string[]): QueueSelection => ({
  selected: new Set(selected),
  anchor: selected[0],
});
test('queue click replaces selection; control toggles and updates the range anchor', () => {
  let state = selectQueueRows(ids, selection('a', 'c'), 'b');
  assert.deepEqual([...state.selected], ['b']);
  state = selectQueueRows(ids, state, 'e', true);
  assert.deepEqual([...state.selected], ['b', 'e']);
  assert.equal(state.anchor, 'e');
  state = selectQueueRows(ids, state, 'e', true);
  assert.deepEqual([...state.selected], ['b']);
});
test('shift replaces with an inclusive range; control-shift subtracts that range', () => {
  const state = selectQueueRows(ids, selection('e', 'a'), 'b', false, true);
  assert.deepEqual([...state.selected], ['b', 'c', 'd', 'e']);
  assert.equal(state.anchor, 'e');
  const subtracted = selectQueueRows(ids, state, 'd', true, true);
  assert.deepEqual([...subtracted.selected], ['b', 'c']);
  assert.equal(subtracted.anchor, 'e');
});
test('a vanished anchor starts a new selection without inventing a range', () => {
  const state = selectQueueRows(ids, selection('removed'), 'b', false, true);
  assert.deepEqual([...state.selected], ['b']);
  const previous = selection('a');
  assert.equal(selectQueueRows(ids, previous, 'missing'), previous);
});
test('queue drag inserts a stable block from display order, regardless of click order', () => {
  assert.deepEqual(reorderQueueRows(ids, new Set(['e', 'b']), 6), ['a', 'c', 'd', 'f', 'b', 'e']);
  assert.deepEqual(reorderQueueRows(ids, new Set(['e', 'b']), 0), ['b', 'e', 'a', 'c', 'd', 'f']);
  assert.deepEqual(reorderQueueRows(ids, new Set(['e', 'b']), 3), ['a', 'c', 'b', 'e', 'd', 'f']);
});
test('dragging a contiguous group onto itself or all entries is a no-op', () => {
  for (const boundary of [1, 2, 3])
    assert.equal(reorderQueueRows(ids, new Set(['b', 'c']), boundary), undefined);
  for (let boundary = 0; boundary <= ids.length; boundary++)
    assert.equal(reorderQueueRows(ids, new Set(ids), boundary), undefined);
});
test('sorting rejects invalid boundaries, duplicate identities and vanished entries', () => {
  for (const boundary of [-1, 7, NaN, 1.5])
    assert.equal(reorderQueueRows(ids, new Set(['b']), boundary), undefined);
  assert.equal(reorderQueueRows(['a', 'a'], new Set(['a']), 1), undefined);
  assert.equal(reorderQueueRows(ids, new Set(['vanished']), 2), undefined);
  assert.equal(reorderQueueRows(ids, new Set(), 2), undefined);
});
test('every subset and boundary preserves the complete entry permutation and stable order', () => {
  for (let mask = 1; mask < 1 << ids.length; mask++) {
    const picked = new Set(ids.filter((_, i) => mask & (1 << i)));
    for (let boundary = 0; boundary <= ids.length; boundary++) {
      const result = reorderQueueRows(ids, picked, boundary) ?? ids;
      assert.deepEqual([...result].sort(), ids);
      assert.deepEqual(
        result.filter((id) => picked.has(id)),
        ids.filter((id) => picked.has(id)),
      );
      assert.deepEqual(
        result.filter((id) => !picked.has(id)),
        ids.filter((id) => !picked.has(id)),
      );
    }
  }
});
