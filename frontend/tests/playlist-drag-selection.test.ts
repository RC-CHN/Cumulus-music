import assert from 'node:assert/strict';
import test from 'node:test';
import {
  emptyPlaylistDragSelection,
  playlistRowDown,
  playlistRowRelease,
  playlistSelectionNeighbor,
} from '../src/services/playlistDragSelection.ts';

const rows = ['A', 'B', 'C', 'D'];

test('Ctrl preserves gesture order, plain down on a selected row preserves the group', () => {
  let state = playlistRowDown(emptyPlaylistDragSelection(), rows, 'D', false, false, false);
  state = playlistRowDown(state, rows, 'B', true, false, false);
  assert.deepEqual([...state.ids], ['D', 'B']);
  assert.strictEqual(playlistRowDown(state, rows, 'B', false, false, false), state);
  assert.deepEqual([...playlistRowRelease('B').ids], ['B']);
  assert.deepEqual([...playlistRowDown(state, rows, 'A', false, false, false).ids], ['A']);
});

test('Shift orders the source range and Ctrl+Shift removes it', () => {
  let state = playlistRowDown(emptyPlaylistDragSelection(), rows, 'D', false, false, false);
  state = playlistRowDown(state, rows, 'B', false, true, false);
  assert.deepEqual([...state.ids], ['B', 'C', 'D']);
  assert.equal(state.anchor, 'D');
  state = playlistRowDown(state, rows, 'C', true, true, false);
  assert.deepEqual([...state.ids], ['B']);
});

test('arrows use the displayed edge of a disjoint selection', () => {
  const selected = new Set(['C', 'B']);
  assert.equal(playlistSelectionNeighbor(rows, selected, -1), 0);
  assert.equal(playlistSelectionNeighbor(rows, selected, 1), 3);
  assert.equal(playlistSelectionNeighbor(rows, new Set(), -1), undefined);
});
