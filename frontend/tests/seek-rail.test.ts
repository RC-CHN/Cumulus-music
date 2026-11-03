import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Timeline } from '../src/services/player.ts';
import { insidePlayableTrial, seekRail } from '../src/services/seekRail.ts';

const trial: Timeline = {
  mode: 'trial',
  start: 20,
  end: 40,
  length: 20,
  total: 100,
  media_start: 0,
};

test('partial trial uses full-song geometry but only accepts native excerpt bounds', () => {
  assert.deepEqual(seekRail(trial), {
    min: 0,
    max: 100,
    span: { left: 20, right: 40 },
    trial: { start: 20, end: 40 },
  });
  for (const allowed of [20, 30, 40]) assert.equal(insidePlayableTrial(trial, allowed), true);
  for (const rejected of [10, 19.9, 40.1, 70, Infinity, NaN])
    assert.equal(insidePlayableTrial(trial, rejected), false);
});

test('unknown total does not invent a full rail or relax rights; full and CUE stay native', () => {
  for (const total of [null, 30, NaN]) {
    const unknown = { ...trial, total };
    assert.deepEqual(seekRail(unknown), {
      min: 20,
      max: 40,
      span: null,
      trial: { start: 20, end: 40 },
    });
    assert.equal(insidePlayableTrial(unknown, 10), false);
  }
  for (const mode of ['full', 'full_trial', 'cue'] as const) {
    const other = { ...trial, mode, start: 0, end: 40, total: 40 };
    assert.deepEqual(seekRail(other), { min: 0, max: 40, span: null, trial: null });
    assert.equal(insidePlayableTrial(other, 10), true);
  }
});
