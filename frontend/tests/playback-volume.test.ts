import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, ref, shallowRef } from 'vue';
import { usePlaybackVolume } from '../src/composables/usePlaybackVolume.ts';
import type { PlayerModel } from '../src/composables/usePlayer.ts';
import type { PlayerView } from '../src/services/player.ts';
const turn = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve!: () => void, reject!: (error: unknown) => void;
  const promise = new Promise<void>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { promise, resolve, reject };
}
function fixture(save: (value: number, epoch: string) => Promise<void> = async () => {}) {
  const view = shallowRef({ epoch: '1', volume: 70, unmuteVolume: 70 } as PlayerView);
  const ready = ref(true),
    busy = ref(false),
    calls: number[] = [];
  const player = {
    view,
    ready,
    busy,
    configureVolume: async (value: number) => {
      const epoch = view.value.epoch;
      calls.push(value);
      await save(value, epoch);
      if (view.value.epoch !== epoch) throw { code: 'stale', message: 'changed', retryable: false };
      view.value = {
        ...view.value,
        volume: value,
        unmuteVolume: value > 0 ? value : view.value.unmuteVolume,
      };
      return view.value;
    },
  } as unknown as PlayerModel;
  const scope = effectScope();
  return {
    view,
    ready,
    busy,
    calls,
    model: scope.run(() => usePlaybackVolume(player))!,
    stop: () => scope.stop(),
  };
}
test('volume input is bounded and unchanged/nonfinite values do not write', async () => {
  const f = fixture();
  f.model.set(70);
  f.model.set(NaN);
  f.model.set(Infinity);
  assert.deepEqual(f.calls, []);
  f.model.set(200);
  await turn();
  assert.equal(f.view.value.volume, 100);
  f.model.wheel(-1);
  await turn();
  assert.deepEqual(f.calls, [100]);
  f.model.wheel(1);
  await turn();
  assert.equal(f.view.value.volume, 97);
  f.model.set(-20);
  await turn();
  assert.equal(f.view.value.volume, 0);
  f.model.wheel(0);
  await turn();
  assert.deepEqual(f.calls, [100, 97, 0]);
  f.stop();
});
test('rapid drag updates serialize and coalesce to the latest absolute target without transport busy', async () => {
  const first = deferred(),
    second = deferred();
  const f = fixture((value) => (value === 41 ? first.promise : second.promise));
  f.model.begin();
  f.model.set(41);
  f.model.set(42);
  f.model.set(0);
  f.model.set(55);
  assert.deepEqual(f.calls, [41]);
  assert.equal(f.model.value.value, 55);
  assert.equal(f.busy.value, false);
  first.resolve();
  await turn();
  assert.deepEqual(f.calls, [41, 55]);
  assert.equal(f.model.value.value, 55);
  assert.equal(f.model.pending.value, true);
  f.model.end();
  second.resolve();
  await turn();
  assert.equal(f.model.value.value, 55);
  assert.equal(f.model.pending.value, false);
  f.stop();
});
test('repeated mute intent during an in-flight save restores the optimistic nonzero value', async () => {
  const saved = deferred();
  const f = fixture((value) => (value === 0 ? saved.promise : Promise.resolve()));
  f.model.set(42);
  await turn();
  f.model.toggleMute();
  f.model.toggleMute();
  assert.equal(f.model.value.value, 42);
  assert.deepEqual(f.calls, [42, 0]);
  saved.resolve();
  await turn();
  assert.deepEqual(f.calls, [42, 0, 42]);
  assert.equal(f.view.value.volume, 42);
  f.stop();
});
test('failure discards queued writes and retries only the latest explicit absolute value', async () => {
  const blocked = deferred();
  let fail = true;
  const f = fixture(() => (fail ? blocked.promise : Promise.resolve()));
  f.model.set(0);
  f.model.set(42);
  blocked.reject({ code: 'storage', message: '合成音量保存失败', retryable: true });
  await turn();
  assert.deepEqual(f.calls, [0]);
  assert.equal(f.model.value.value, 70);
  assert.equal(f.model.issue.value?.code, 'storage');
  await turn();
  assert.deepEqual(f.calls, [0]);
  fail = false;
  f.model.retry();
  await turn();
  assert.deepEqual(f.calls, [0, 42]);
  assert.equal(f.model.issue.value, undefined);
  f.stop();
});
test('account changes discard old coalesced work without clearing a newer pending request', async () => {
  const old = deferred(),
    fresh = deferred();
  const f = fixture((_value, epoch) => (epoch === '1' ? old.promise : fresh.promise));
  f.model.set(0);
  f.model.set(42);
  f.view.value = { ...f.view.value, epoch: '2', volume: 20, unmuteVolume: 20 };
  f.model.set(30);
  old.resolve();
  await turn();
  assert.deepEqual(f.calls, [0, 30]);
  assert.equal(f.model.value.value, 30);
  assert.equal(f.model.pending.value, true);
  assert.equal(f.model.issue.value, undefined);
  fresh.resolve();
  await turn();
  assert.equal(f.view.value.volume, 30);
  assert.equal(f.model.pending.value, false);
  f.stop();
});
test('unmount or disconnect stops queued writes and late feedback', async () => {
  for (const dispose of [true, false]) {
    const saved = deferred(),
      f = fixture(() => saved.promise);
    f.model.set(0);
    f.model.set(42);
    if (dispose) f.stop();
    else f.ready.value = false;
    saved.reject({ code: 'storage', message: 'late', retryable: true });
    await turn();
    assert.deepEqual(f.calls, [0]);
    assert.equal(f.model.pending.value, false);
    assert.equal(f.model.issue.value, undefined);
    f.stop();
  }
});
test('external native volume updates do not move a held slider and reconcile on release', () => {
  const f = fixture();
  f.model.begin();
  f.view.value = { ...f.view.value, volume: 20, unmuteVolume: 20 };
  assert.equal(f.model.value.value, 70);
  f.model.end();
  assert.equal(f.model.value.value, 20);
  f.stop();
});
test('unmute uses the native durable value after an external zero-volume update', async () => {
  const f = fixture();
  f.view.value = { ...f.view.value, volume: 0, unmuteVolume: 27 };
  f.model.toggleMute();
  await turn();
  assert.deepEqual(f.calls, [27]);
  assert.equal(f.model.value.value, 27);
  f.stop();
});
