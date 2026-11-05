import assert from 'node:assert/strict';
import test from 'node:test';
import {
  captureSearchPresentationViewport,
  restoreSearchPresentationViewport,
} from '../src/services/searchPresentationViewport.ts';

test('presentation keeps surviving focus and scroll, replacing only a removed control', (t) => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'document');
  const body = {},
    outside = {};
  const doc = { body, activeElement: undefined as unknown };
  Object.defineProperty(globalThis, 'document', { configurable: true, value: doc });
  t.after(() => {
    if (previous) Object.defineProperty(globalThis, 'document', previous);
    else Reflect.deleteProperty(globalThis, 'document');
  });
  const calls: unknown[] = [];
  const section = { dataset: { resultSection: 'ordinary' }, querySelectorAll: () => [card] };
  let controls: unknown[];
  const card = {
    dataset: { resultKey: 'song:1' },
    closest: () => section,
    querySelectorAll: () => controls,
  };
  const node = (action: string) => ({
    dataset: { resultAction: action },
    isConnected: true,
    closest: (selector: string) => (selector === '[data-result-key]' ? card : null),
    getClientRects: () => [1],
    focus: (options: unknown) => {
      calls.push([action, options]);
      doc.activeElement = action;
    },
  });
  const oldReason = node('reason:0'),
    cover = node('cover');
  controls = [cover, oldReason];
  const panel = {
    scrollTop: 345,
    contains: (node: unknown) => node === oldReason || node === cover,
    querySelectorAll: () => [section],
  } as unknown as HTMLElement;
  doc.activeElement = oldReason;
  const saved = captureSearchPresentationViewport(panel);
  panel.scrollTop = 290;
  restoreSearchPresentationViewport(panel, saved);
  assert.equal(panel.scrollTop, 345);
  assert.deepEqual(calls, [], 'a connected keyed node must retain its own focus');

  oldReason.isConnected = false;
  controls = [cover];
  doc.activeElement = outside;
  restoreSearchPresentationViewport(panel, saved);
  assert.deepEqual(calls, [], 'do not steal focus from a newer user target');
  doc.activeElement = body;
  restoreSearchPresentationViewport(panel, saved);
  assert.deepEqual(calls, [['cover', { preventScroll: true }]]);
  assert.equal(panel.scrollTop, 345);
});

test('a removed third best-match card falls back only inside its own section', (t) => {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'document');
  const body = {},
    doc = { body: {}, activeElement: undefined as unknown };
  doc.body = body;
  Object.defineProperty(globalThis, 'document', { configurable: true, value: doc });
  t.after(() => {
    if (previous) Object.defineProperty(globalThis, 'document', previous);
    else Reflect.deleteProperty(globalThis, 'document');
  });
  let focused = '';
  const control = {
    dataset: {},
    getClientRects: () => [1],
    closest: () => null,
    focus: () => {
      focused = 'best-first';
    },
  };
  const first = { dataset: { resultKey: 'artist:1' }, querySelectorAll: () => [control] };
  const section = { dataset: { resultSection: 'best' }, querySelectorAll: () => [first] };
  const panel = { scrollTop: 123, querySelectorAll: () => [section] } as unknown as HTMLElement;
  const saved = {
    top: 123,
    focused: { isConnected: false } as HTMLElement,
    sectionKey: 'best',
    cardKey: 'artist:3',
    action: 'more',
  };
  doc.activeElement = body;
  restoreSearchPresentationViewport(panel, saved);
  assert.equal(focused, 'best-first');
  assert.equal(panel.scrollTop, 123);
});
