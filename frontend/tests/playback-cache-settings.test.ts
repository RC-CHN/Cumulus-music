import assert from 'node:assert/strict';
import { test } from 'node:test';
import { usePlaybackCache } from '../src/composables/usePlaybackCache.ts';
import {
  playbackCacheQuota,
  playbackCacheUsage,
  type PlaybackCacheStatus,
} from '../src/services/playbackCache.ts';

const step = 512 * 1024 ** 2;
const start: PlaybackCacheStatus = {
  revision: '18446744073709551613',
  directory: '/synthetic/private/streaming-audio-v1',
  capacityBytes: step * 2,
  minCapacityBytes: step,
  maxCapacityBytes: step * 20,
  capacityStepBytes: step,
  occupiedBytes: 8192,
  reservedBytes: 16384,
  reclaimableBytes: 4096,
  overCapacity: false,
};

test('range preview is local, and a committed valid capacity uses only one device revision', async () => {
  let reads = 0;
  let finish!: (snapshot: PlaybackCacheStatus) => void;
  const sends: [string, number][] = [];
  const model = usePlaybackCache({
    playbackCacheState: async () => {
      reads++;
      return start;
    },
    playbackCacheSetCapacity: (revision, bytes) => {
      sends.push([revision, bytes]);
      return new Promise((resolve) => {
        finish = resolve;
      });
    },
    playbackCacheClear: async () => {
      throw new Error('clear must remain fenced during save');
    },
  });
  await model.read();
  model.preview(step);
  assert.equal(model.capacity.value, step);
  model.preview(step * 3);
  model.preview(step * 3 + 1);
  assert.equal(model.capacity.value, step * 3);
  await model.setCapacity(step * 3 + 1);
  assert.deepEqual(sends, []);
  assert.equal(model.view.value?.capacityBytes, step * 2);
  const saving = model.setCapacity(step * 3);
  assert.equal(model.pending.value, 'capacity');
  await model.setCapacity(step * 4);
  await model.read();
  await model.clear();
  assert.equal(reads, 1);
  assert.deepEqual(sends, [[start.revision, step * 3]]);
  finish({ ...start, revision: '18446744073709551614', capacityBytes: step * 3 });
  await saving;
  assert.equal(model.result.value, 'capacity');
  assert.equal(model.capacity.value, step * 3);
  await model.setCapacity(step * 3);
  assert.equal(sends.length, 1);
});

test('failed clear reads the actual remaining usage once and never repeats the write', async () => {
  const after = {
    ...start,
    revision: '17',
    reclaimableBytes: 0,
    reservedBytes: 8192,
    occupiedBytes: 4096,
  };
  let reads = 0;
  const sends: string[] = [];
  const model = usePlaybackCache({
    playbackCacheState: async () => (++reads === 1 ? start : after),
    playbackCacheSetCapacity: async () => {
      throw new Error('no capacity change');
    },
    playbackCacheClear: async (revision) => {
      sends.push(revision);
      throw new Error('synthetic lost IPC result after native clear');
    },
  });
  await model.read();
  await model.clear();
  assert.deepEqual(sends, [start.revision]);
  assert.equal(reads, 2);
  assert.equal(model.view.value, after);
  assert.equal(model.result.value, undefined);
  assert.match(model.issue.value, /已重新读取/);
});

test('explicit clear remains available at zero usage and advances the observed native revision', async () => {
  const empty = { ...start, occupiedBytes: 0, reservedBytes: 0, reclaimableBytes: 0 };
  const cleared = { ...empty, revision: '18446744073709551614' };
  const sends: string[] = [];
  const model = usePlaybackCache({
    playbackCacheState: async () => empty,
    playbackCacheSetCapacity: async () => {
      throw new Error('no capacity change');
    },
    playbackCacheClear: async (revision) => {
      sends.push(revision);
      return cleared;
    },
  });
  await model.read();
  assert.equal(model.canClear.value, true);
  await model.clear();
  assert.deepEqual(sends, [empty.revision]);
  assert.equal(model.result.value, 'clear');
  assert.equal(model.view.value, cleared);
});

test('unreadable or unavailable native storage fences all cache changes', async () => {
  const responses = [start, undefined, null];
  const sends: string[] = [];
  const model = usePlaybackCache({
    playbackCacheState: async () => {
      const value = responses.shift();
      if (value === undefined) throw new Error('fixture read failed');
      return value;
    },
    playbackCacheSetCapacity: async () => {
      throw new Error('must be fenced');
    },
    playbackCacheClear: async (revision) => {
      sends.push(revision);
      throw new Error('fixture mutation failed');
    },
  });
  await model.read();
  await model.clear();
  assert.equal(model.stale.value, true);
  assert.equal(model.canChange.value, false);
  await model.setCapacity(step * 3);
  await model.clear();
  assert.deepEqual(sends, [start.revision]);
  await model.read();
  assert.equal(model.view.value, null);
  assert.equal(model.stale.value, false);
  assert.equal(model.canClear.value, false);
});

test('a departed settings page discards a pending reply and does not issue recovery IPC', async () => {
  let reads = 0;
  let fail!: () => void;
  const model = usePlaybackCache({
    playbackCacheState: async () => {
      reads++;
      return start;
    },
    playbackCacheSetCapacity: async () => start,
    playbackCacheClear: () =>
      new Promise((_resolve, reject) => {
        fail = () => reject(new Error('late failure'));
      }),
  });
  await model.read();
  const clear = model.clear();
  model.dispose();
  fail();
  await clear;
  assert.equal(reads, 1);
  assert.equal(model.result.value, undefined);
  assert.equal(model.issue.value, '');
});

test('quota uses official fractional G/512M and actual usage does not round to whole megabytes', () => {
  assert.equal(playbackCacheQuota(step), '512M');
  assert.equal(playbackCacheQuota(step * 3), '1.5G');
  assert.equal(playbackCacheQuota(step * 3, true), '1.5 GiB');
  assert.equal(playbackCacheUsage(1025), '1 KiB');
  assert.equal(playbackCacheUsage(2048), '2 KiB');
  assert.equal(playbackCacheUsage(0), '0 字节');
});
