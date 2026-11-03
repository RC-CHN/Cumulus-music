import assert from 'node:assert/strict';
import test from 'node:test';
import { searchSuggestionIndex } from '../src/services/searchKeyboard.ts';

test('suggestion arrows enter at opposite edges and wrap without dropping focus', () => {
  assert.equal(searchSuggestionIndex(7, -1, 'ArrowDown'), 0);
  assert.equal(searchSuggestionIndex(7, -1, 'ArrowUp'), 6);
  assert.equal(searchSuggestionIndex(7, 0, 'ArrowUp'), 6);
  assert.equal(searchSuggestionIndex(7, 6, 'ArrowDown'), 0);
  assert.equal(searchSuggestionIndex(7, 3, 'ArrowDown'), 4);
  assert.equal(searchSuggestionIndex(7, 3, 'ArrowUp'), 2);
});

test('single and empty suggestion lists cannot produce an invalid focus target', () => {
  assert.equal(searchSuggestionIndex(0, -1, 'ArrowDown'), undefined);
  assert.equal(searchSuggestionIndex(1, 0, 'ArrowDown'), 0);
  assert.equal(searchSuggestionIndex(1, 0, 'ArrowUp'), 0);
  assert.equal(searchSuggestionIndex(7, 4, 'Home'), 0);
  assert.equal(searchSuggestionIndex(7, 4, 'End'), 6);
});
