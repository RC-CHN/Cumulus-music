import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  allVoiceSongs,
  singleVoiceSong,
  voiceSongContext,
  voiceSongDown,
  voiceSongNeighbor,
  voiceSongSelectionSnapshot,
  voiceSongUp,
} from '../src/services/voiceSongSelection.ts';

const plain = { button: 0, toggle: false, range: false };
const ctrl = { ...plain, toggle: true };
const shift = { ...plain, range: true };
const right = { ...plain, button: 2 };

test('official modifier order survives a selected right-click and freezes the menu target', () => {
  const first = singleVoiceSong(8);
  const group = voiceSongDown(first, 2, 10, ctrl);
  const pressed = voiceSongDown(group, 2, 10, right);
  const menu = voiceSongContext(pressed, 2, false);
  const snapshot = voiceSongSelectionSnapshot(menu, 2);
  assert.deepEqual(snapshot, [8, 2], 'dispatch order is not displayed order');
  assert(Object.isFrozen(snapshot));
  const nextMenu = voiceSongContext(voiceSongDown(menu, 4, 10, right), 4, false);
  assert.deepEqual([...nextMenu.indices], [4]);
  assert.equal(voiceSongSelectionSnapshot(nextMenu, 4), undefined);
  assert.deepEqual(snapshot, [8, 2], 'later selection cannot mutate a menu already open');
  const outside = voiceSongContext(menu, 4, true);
  assert.deepEqual([...outside.indices], [8, 2], 'modifier right-click does not collapse');
  assert.equal(
    voiceSongSelectionSnapshot(outside, 4),
    undefined,
    'an unselected row cannot anchor a group',
  );
});

test('plain press defers collapse to release; Shift replaces and Ctrl+Shift subtracts', () => {
  let state = voiceSongDown(singleVoiceSong(8), 2, 10, ctrl);
  const pressed = voiceSongDown(state, 8, 10, plain);
  assert.deepEqual([...pressed.indices], [8, 2]);
  assert.deepEqual([...voiceSongUp(pressed, 8, plain).indices], [8]);
  state = voiceSongDown(state, 5, 10, shift);
  assert.deepEqual([...state.indices], [2, 3, 4, 5]);
  state = voiceSongDown(state, 3, 10, { ...ctrl, range: true });
  assert.deepEqual([...state.indices], [4, 5]);
  assert.equal(state.anchor, 2);
  const modifiedRelease = voiceSongUp(voiceSongDown(state, 5, 10, plain), 5, ctrl);
  assert.deepEqual([...modifiedRelease.indices], [4, 5]);
});

test('keyboard selection addresses offscreen ordinals and arrows leave the full range', () => {
  const distant = voiceSongDown(singleVoiceSong(7), 96, 100, ctrl);
  assert.equal(voiceSongNeighbor(distant, 7, 100, -1), 6);
  assert.equal(voiceSongNeighbor(distant, 7, 100, 1), 97);
  const all = allVoiceSongs(distant, 100);
  const snapshot = voiceSongSelectionSnapshot(all, 96);
  assert.equal(snapshot?.length, 100);
  assert.equal(snapshot?.[0], 0);
  assert.equal(snapshot?.[99], 99);
  assert.equal(voiceSongNeighbor(all, 7, 100, -1), 0);
  assert.equal(voiceSongNeighbor(all, 7, 100, 1), 99);
});
