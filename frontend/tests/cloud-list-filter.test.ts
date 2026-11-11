import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, shallowRef } from 'vue';
import { useCloudListFilter } from '../src/composables/useCloudListFilter.ts';
import type { CloudTab } from '../src/services/cloud.ts';

test('cloud input applies normal typing after 10 ms and IME edits immediately', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const effect = effectScope();
  const state = shallowRef({ epoch: '1', tab: 'uploaded' as CloudTab, keyword: '' });
  const committed: string[] = [];
  const filter = effect.run(() =>
    useCloudListFilter(
      () => state.value,
      (keyword) => {
        committed.push(keyword);
        state.value = { ...state.value, keyword };
      },
    ),
  )!;
  try {
    filter.change('a');
    t.mock.timers.tick(9);
    filter.change('Ａ＋Ｂ');
    t.mock.timers.tick(9);
    assert.deepEqual(committed, []);
    assert.equal(filter.keyword.value, 'Ａ＋Ｂ');
    t.mock.timers.tick(1);
    filter.submit();
    assert.deepEqual(committed, ['Ａ＋Ｂ']);

    filter.change('old Latin draft');
    filter.startComposition();
    filter.change('樂');
    filter.change('樂隊');
    filter.submit();
    t.mock.timers.tick(10);
    filter.endComposition('樂隊 夜場');
    filter.change('樂隊 夜場');
    t.mock.timers.tick(10);
    assert.deepEqual(committed, ['Ａ＋Ｂ', '樂', '樂隊', '樂隊 夜場']);
    filter.clear();
    assert.equal(committed.at(-1), '');
  } finally {
    effect.stop();
  }
});

test('debounced cloud input cannot escape its account, tab or mounted visit', (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const effect = effectScope();
  const state = shallowRef({ epoch: '1', tab: 'uploaded' as CloudTab, keyword: 'committed' });
  const calls: string[] = [];
  const filter = effect.run(() =>
    useCloudListFilter(
      () => state.value,
      (value) => calls.push(value),
    ),
  )!;
  try {
    filter.change('private old owner draft');
    state.value = { epoch: '2', tab: 'uploaded', keyword: '' };
    t.mock.timers.tick(10);
    assert.equal(filter.keyword.value, '');
    filter.change('upload tab pending draft');
    state.value = { ...state.value, tab: 'uploading' };
    state.value = { ...state.value, tab: 'uploaded', keyword: 'restored' };
    t.mock.timers.tick(10);
    assert.equal(filter.keyword.value, 'restored');
    filter.change('pending at close');
    effect.stop();
    t.mock.timers.tick(10);
    assert.deepEqual(calls, []);
  } finally {
    effect.stop();
  }
});
