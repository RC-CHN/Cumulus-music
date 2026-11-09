import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { Timeline } from '../src/services/player.ts';
import {
  bufferedSeekSpan,
  chorusDisplayClock,
  chorusMarkerPercent,
  insidePlayableTrial,
  seekRail,
} from '../src/services/seekRail.ts';

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

test('native buffering appears ahead of the clock and never beyond the authorized trial', () => {
  assert.deepEqual(bufferedSeekSpan(trial, 28, 60), { left: 28, right: 40 });
  assert.deepEqual(bufferedSeekSpan({ ...trial, total: null }, 28, 36), { left: 40, right: 80 });
  const full: Timeline = { ...trial, mode: 'full', start: 0, end: 100 };
  assert.deepEqual(bufferedSeekSpan(full, 20, 30), { left: 20, right: 30 });
  for (const unknown of [null, undefined, 15, NaN, Infinity])
    assert.equal(bufferedSeekSpan(full, 20, unknown), null);
  assert.equal(bufferedSeekSpan({ ...full, end: null }, 20, 30), null);
  assert.equal(bufferedSeekSpan(trial, 40, 60), null);
});

test('chorus metadata annotates the visible clock without expanding trial authorization', () => {
  const full: Timeline = { ...trial, mode: 'full', start: 0, end: 100 };
  assert.equal(chorusMarkerPercent(full, 45), 45);
  assert.equal(chorusMarkerPercent(trial, 10), 10);
  assert.equal(insidePlayableTrial(trial, 10), false);
  assert.equal(chorusMarkerPercent(trial, 40), null);
  assert.equal(chorusMarkerPercent({ ...trial, total: null }, 25), 25);
  assert.equal(chorusMarkerPercent({ ...trial, total: null }, 10), null);
  for (const bad of [null, 0, -1, 100, 101, Infinity, NaN])
    assert.equal(chorusMarkerPercent(full, bad), null);
  assert.equal(chorusMarkerPercent({ ...full, mode: 'cue' }, 45), null);
  assert.equal(chorusMarkerPercent({ ...full, end: null }, 45), null);
});

test('standard chorus clock uses the native whole-second tick on the track or trial clock', () => {
  assert.equal(chorusDisplayClock(4.82, 0), 4);
  assert.equal(chorusDisplayClock(5, 0), 5);
  assert.equal(chorusDisplayClock(14.82, 10.25), 14.25);
  assert.equal(chorusDisplayClock(14.68, 10.25, 14.67), 14.67);
  assert.equal(chorusDisplayClock(14.75, 10.25, 14.75), 14.25);
  assert.equal(chorusDisplayClock(15.33, 10.25, 14.67), 15.25);
  assert.equal(chorusDisplayClock(null, 0), null);
  assert.equal(chorusDisplayClock(Infinity, 0), null);
});
