import { test } from 'node:test';
import assert from 'node:assert/strict';
import { fmPrimary, type FmPrimaryState } from '../src/services/fmPrimary.ts';

const ready: FmPrimaryState = {
  active: true,
  playing: false,
  modePending: false,
  transportActive: true,
  hasCurrent: true,
  pending: false,
  notice: false,
};

test('a page without playback ownership starts its own source and cannot pause other audio', () => {
  assert.deepEqual(fmPrimary({ ...ready, active: false, playing: true }), {
    action: 'start',
    label: '开始漫游',
    icon: 'play',
  });
});
test('a pending empty desired queue does not replace the audible pause intent with a fetch icon', () => {
  assert.deepEqual(
    fmPrimary({ ...ready, playing: true, hasCurrent: false, pending: true, notice: true }),
    {
      action: 'pause',
      label: '暂停漫游',
      icon: 'pause',
    },
  );
});
test('an empty stopped FM source explicitly offers fetching recommendations', () => {
  for (const transportActive of [false, true])
    assert.deepEqual(fmPrimary({ ...ready, hasCurrent: false, transportActive }), {
      action: 'resume',
      label: '重新获取推荐',
      icon: 'repeat',
    });
});
test('mode recovery stays explicit even while the old native mode is playing', () => {
  assert.deepEqual(fmPrimary({ ...ready, modePending: true, playing: true }), {
    action: 'resume',
    label: '同步并继续播放',
    icon: 'repeat',
  });
});
test('continuing uses the coordinator for an unsynchronized source and native play only when ready', () => {
  for (const state of [
    { ...ready, transportActive: false },
    { ...ready, pending: true },
    { ...ready, notice: true },
  ])
    assert.deepEqual(fmPrimary(state), { action: 'resume', label: '继续播放', icon: 'play' });
  assert.deepEqual(fmPrimary(ready), { action: 'play', label: '继续播放', icon: 'play' });
});
