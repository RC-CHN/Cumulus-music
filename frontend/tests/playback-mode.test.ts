import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { effectScope, ref, shallowRef } from 'vue';
import { usePlaybackMode } from '../src/composables/usePlaybackMode.ts';
import { nextMode, type ModeFamily } from '../src/services/playbackMode.ts';
import type { PlayerModel } from '../src/composables/usePlayer.ts';
import type { PlaybackMode, PlayerView } from '../src/services/player.ts';
const turn = () => new Promise((r) => setImmediate(r));
function deferred() {
  let resolve!: () => void, reject!: (e: unknown) => void;
  const promise = new Promise<void>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { resolve, reject, promise };
}
function fixture(save: (mode: PlaybackMode) => Promise<void> = async () => {}) {
  const view = shallowRef({
      epoch: '1',
      queueRevision: '1',
      mode: 'order_loop',
      fm: false,
    } as PlayerView),
    ready = ref(true),
    busy = ref(false),
    together = ref(false);
  const calls: { mode: PlaybackMode; revision: string; fm: boolean }[] = [];
  const player = {
    view,
    ready,
    busy,
    configureMode: async (mode: PlaybackMode, revision: string, fm: boolean) => {
      const epoch = view.value.epoch;
      calls.push({ mode, revision, fm });
      await save(mode);
      if (view.value.epoch !== epoch) throw { code: 'stale', message: 'changed', retryable: false };
      if (view.value.queueRevision !== revision || view.value.fm !== fm)
        throw { code: 'queue_changed', message: 'queue changed', retryable: true };
      view.value = { ...view.value, mode, queueRevision: String(BigInt(revision) + 1n) };
      return view.value;
    },
  } as unknown as PlayerModel;
  const scope = effectScope();
  return {
    view,
    ready,
    busy,
    together,
    calls,
    model: scope.run(() => usePlaybackMode(player, () => together.value))!,
    stop: () => scope.stop(),
  };
}
test('recovered non-heartbeat, together and FM rings have distinct next modes', () => {
  const frozen = JSON.parse(
    readFileSync(
      new URL('../../core/tests/fixtures/official-playback-mode.json', import.meta.url),
      'utf8',
    ),
  ) as { vectors: { family: ModeFamily; current: PlaybackMode; next: PlaybackMode }[] };
  assert.equal(frozen.vectors.length, 12);
  for (const row of frozen.vectors) assert.equal(nextMode(row.current, row.family), row.next);
  assert.deepEqual(
    ['sequential', 'order_loop', 'single_loop', 'random'].map((m) =>
      nextMode(m as PlaybackMode, 'ordinary'),
    ),
    ['order_loop', 'single_loop', 'random', 'sequential'],
  );
  assert.deepEqual(
    ['order_loop', 'single_loop', 'random'].map((m) => nextMode(m as PlaybackMode, 'together')),
    ['single_loop', 'random', 'order_loop'],
  );
  assert.equal(nextMode('sequential', 'together'), 'single_loop');
  assert.equal(nextMode('sequential', 'fm'), 'single_loop');
  assert.equal(nextMode('single_loop', 'fm'), 'sequential');
});
test('rapid cycles serialize first and latest intent with fresh queue revisions without transport busy', async () => {
  const saved = deferred(),
    f = fixture((m) => (m === 'single_loop' ? saved.promise : Promise.resolve()));
  f.busy.value = true;
  f.model.cycle();
  f.model.cycle();
  f.model.cycle();
  assert.equal(f.model.value.value, 'sequential');
  assert.equal(f.model.actual.value, 'order_loop');
  assert.equal(f.model.pending.value, true);
  assert.equal(f.busy.value, true);
  assert.deepEqual(f.calls, [{ mode: 'single_loop', revision: '1', fm: false }]);
  saved.resolve();
  await turn();
  assert.deepEqual(f.calls, [
    { mode: 'single_loop', revision: '1', fm: false },
    { mode: 'sequential', revision: '2', fm: false },
  ]);
  assert.equal(f.view.value.mode, 'sequential');
  assert.equal(f.model.pending.value, false);
  f.stop();
});
test('failed bursts stop automatically and explicit retry uses latest desired mode on the current queue', async () => {
  const saved = deferred();
  let fail = true;
  const f = fixture(() => (fail ? saved.promise : Promise.resolve()));
  f.model.cycle();
  f.model.cycle();
  saved.reject({ code: 'storage', message: 'save failed', retryable: true });
  await turn();
  assert.equal(f.calls.length, 1);
  assert.equal(f.model.value.value, 'order_loop');
  assert.equal(f.model.issue.value?.code, 'storage');
  fail = false;
  f.model.retry();
  await turn();
  assert.deepEqual(f.calls[1], { mode: 'random', revision: '1', fm: false });
  assert.equal(f.model.issue.value, undefined);
  f.stop();
});
test('an independent mode recovery clears the obsolete failed target and retry', async () => {
  const f = fixture(async () => {
    throw { code: 'storage', message: 'save failed', retryable: true };
  });
  f.model.cycle();
  await turn();
  assert.equal(f.model.canRetry.value, true);
  f.view.value = { ...f.view.value, mode: 'single_loop', queueRevision: '2' };
  assert.equal(f.model.issue.value, undefined);
  assert.equal(f.model.canRetry.value, false);
  f.model.retry();
  await turn();
  assert.equal(f.calls.length, 1);
  assert.equal(f.model.value.value, 'single_loop');
  f.stop();
});
test('FM retry reaches native reconciliation even when target equals the audible mode', async () => {
  const saved = deferred();
  let fail = true;
  const f = fixture(() => (fail ? saved.promise : Promise.resolve()));
  f.view.value = { ...f.view.value, fm: true, mode: 'sequential' };
  f.model.cycle();
  f.model.cycle();
  saved.reject({ code: 'storage', message: 'apply failed', retryable: true });
  await turn();
  fail = false;
  f.model.retry();
  await turn();
  assert.deepEqual(
    f.calls.map((c) => c.mode),
    ['single_loop', 'sequential'],
  );
  assert.equal(f.calls[1]?.fm, true);
  f.stop();
});
test('a delayed queue revision after a failed durable mode commit preserves explicit retry', async () => {
  let fail = true;
  const f = fixture(async () => {
    if (fail) throw { code: 'storage', message: 'save failed', retryable: true };
  });
  f.model.cycle();
  await turn();
  assert.equal(f.model.canRetry.value, true);
  // Native queue fences advance even when SQLite rejects the candidate; a
  // later state subscription must not erase the RPC failure/retry target.
  f.view.value = { ...f.view.value, queueRevision: '2' };
  assert.equal(f.model.issue.value?.code, 'storage');
  assert.equal(f.model.canRetry.value, true);
  fail = false;
  f.model.retry();
  await turn();
  assert.deepEqual(f.calls[1], { mode: 'single_loop', revision: '2', fm: false });
  assert.equal(f.model.issue.value, undefined);
  f.stop();
});
test('a replaced queue rejects pending mode work without automatic replay', async () => {
  const saved = deferred(),
    f = fixture(() => saved.promise);
  f.model.cycle();
  f.model.cycle();
  f.view.value = { ...f.view.value, queueRevision: '99' };
  saved.resolve();
  await turn();
  assert.equal(f.calls.length, 1);
  assert.equal(f.view.value.mode, 'order_loop');
  assert.equal(f.model.issue.value?.code, 'queue_changed');
  f.stop();
});
test('account and player-family changes discard obsolete mode intentions and errors', async () => {
  for (const change of ['account', 'fm', 'together']) {
    const saved = deferred(),
      f = fixture(() => saved.promise);
    f.model.cycle();
    f.model.cycle();
    if (change === 'account') f.view.value = { ...f.view.value, epoch: '2' };
    else if (change === 'fm') f.view.value = { ...f.view.value, fm: true, mode: 'sequential' };
    else f.together.value = true;
    saved.reject({ code: 'storage', message: 'late', retryable: true });
    await turn();
    assert.equal(f.calls.length, 1);
    assert.equal(f.model.pending.value, false);
    assert.equal(f.model.issue.value, undefined);
    f.stop();
  }
});
test('disconnect and unmount drop coalesced requests and late feedback', async () => {
  for (const dispose of [true, false]) {
    const saved = deferred(),
      f = fixture(() => saved.promise);
    f.model.cycle();
    f.model.cycle();
    if (dispose) f.stop();
    else f.ready.value = false;
    saved.reject({ code: 'storage', message: 'late', retryable: true });
    await turn();
    assert.equal(f.calls.length, 1);
    assert.equal(f.model.issue.value, undefined);
    f.stop();
  }
});
test('together cycling excludes sequential and external state remains authoritative', async () => {
  const f = fixture();
  f.together.value = true;
  for (let i = 0; i < 3; i++) {
    f.model.cycle();
    await turn();
  }
  assert.deepEqual(
    f.calls.map((c) => c.mode),
    ['single_loop', 'random', 'order_loop'],
  );
  f.view.value = { ...f.view.value, mode: 'random' };
  assert.equal(f.model.value.value, 'random');
  f.stop();
});
