import assert from 'node:assert/strict';
import { test } from 'node:test';
import { playlistSelectionNeighbor } from '../src/services/playlistDragSelection.ts';
import {
  emptyLyricSelection,
  lyricGroupIndexes,
  lyricSelectionAll,
  lyricSelectionDown,
  lyricSelectionOnly,
  lyricSelectionUp,
  reconcileLyricSelection,
} from '../src/services/searchLyricSelection.ts';

test('lyrics selection matches the recovered M106 mouse, keyboard and append sequence', () => {
  let displayed = ['a', 'b', 'c', 'c', 'd'];
  let selection = emptyLyricSelection();
  const row = (index: number) => ({ index, id: displayed[index] });
  function down(index: number, button = 0, ctrl = false, shift = false) {
    const result = lyricSelectionDown(selection, displayed, row(index), { button, ctrl, shift });
    selection = result.selection;
    return result.collapse;
  }
  const selected = () => [...selection.ids];
  assert.deepEqual(selected(), []);
  down(3, 0, true);
  down(1, 0, true);
  assert.deepEqual(selected(), ['c', 'b']);
  down(2, 2);
  assert.deepEqual(selected(), ['c', 'b']);
  const collapse = down(2);
  assert.deepEqual(selected(), ['c', 'b']);
  selection = lyricSelectionUp(
    selection,
    row(2),
    { button: 0, ctrl: false, shift: false },
    collapse,
  );
  assert.deepEqual(selected(), ['c']);
  down(0, 0, false, true);
  assert.deepEqual(selected(), ['a', 'b', 'c']);
  down(1, 0, true, true);
  assert.deepEqual(selected(), ['a']);
  selection = lyricSelectionAll(selection, displayed);
  assert.deepEqual(selected(), ['a', 'b', 'c', 'd']);
  displayed = [...displayed, 'e'];
  selection = reconcileLyricSelection(selection, displayed);
  assert.deepEqual(selected(), ['a', 'b', 'c', 'd']);
  const next = playlistSelectionNeighbor(displayed, selection.ids, 1);
  assert.equal(next, 5);
  selection = lyricSelectionOnly(selection, row(next!), false);
  assert.deepEqual(selected(), ['e']);
  selection = reconcileLyricSelection(selection, displayed.slice(0, -1));
  assert.deepEqual(selected(), []);
});

test('Shift uses the exact duplicate hit ordinal rather than its first occurrence', () => {
  const displayed = ['3', '4', '3', '5'];
  const down = lyricSelectionDown(
    emptyLyricSelection(),
    displayed,
    { index: 2, id: '3' },
    { button: 0, ctrl: false, shift: false },
  );
  const range = lyricSelectionDown(
    down.selection,
    displayed,
    { index: 3, id: '5' },
    { button: 0, ctrl: false, shift: true },
  );
  assert.deepEqual([...range.selection.ids], ['3', '5']);
  const outside = lyricSelectionDown(
    range.selection,
    displayed,
    { index: 1, id: '4' },
    { button: 2, ctrl: false, shift: false },
  );
  assert.deepEqual([...outside.selection.ids], ['4']);
  assert.deepEqual(outside.selection.anchor, { index: 1, id: '4' });
});

test('playback group keeps gesture order and uses the clicked hit exactly once', () => {
  const displayed = ['3', '4', '3', '5', '6'];
  const selected = new Set(['5', '3', '4']);
  assert.deepEqual(lyricGroupIndexes(displayed, selected, 2), [3, 2, 1]);
  assert.deepEqual(lyricGroupIndexes(displayed, selected, 0), [3, 0, 1]);
  assert.equal(lyricGroupIndexes(displayed, selected, 4), undefined);
  assert.equal(lyricGroupIndexes(['3', '5'], selected, 0), undefined);
});
