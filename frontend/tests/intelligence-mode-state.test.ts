import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { effectScope, ref, shallowRef } from 'vue';
import { usePlaybackMode } from '../src/composables/usePlaybackMode.ts';
import {
  intelligenceIdle,
  type IntelligenceAttempt,
  type IntelligenceCycle,
} from '../src/composables/intelligenceAttempt.ts';
import { canHeartBeat, nextModeTarget, type ModeTarget } from '../src/services/playbackMode.ts';
import type { PlayerModel } from '../src/composables/usePlayer.ts';
import type { PlayerView, PlaybackMode } from '../src/services/player.ts';
const turn = () => new Promise((r) => setImmediate(r));
function deferred<T>() {
  let resolve!: (v: T) => void, reject!: (e: unknown) => void;
  const promise = new Promise<T>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { promise, resolve, reject };
}
function fixture(save: (mode: PlaybackMode) => Promise<void> = async () => {}) {
  const view = shallowRef({
    epoch: '1',
    revision: '1',
    queueRevision: '1',
    mode: 'sequential',
    fm: false,
    intelligence: null,
    intelligenceOperation: null,
    playId: 'ordinary',
    origin: { kind: 'playlist', id: '31' },
  } as PlayerView);
  const ready = ref(true),
    networkBusy = ref(false),
    transportBusy = ref(false),
    eligible = ref(true),
    together = ref(false);
  const modes: { mode: PlaybackMode; revision: string }[] = [],
    attempts: {
      attempt: IntelligenceAttempt;
      result: ReturnType<typeof deferred<PlayerView>>;
      revision: string;
      cancelled: number;
    }[] = [];
  let snapshots = 0;
  const player = {
    view,
    ready,
    busy: transportBusy,
    syncPlayback: async () => {
      snapshots++;
      return view.value;
    },
    configureMode: async (mode: PlaybackMode, revision: string) => {
      modes.push({ mode, revision });
      const epoch = view.value.epoch;
      await save(mode);
      if (epoch !== view.value.epoch) throw { code: 'stale', message: 'changed', retryable: false };
      if (revision !== view.value.queueRevision)
        throw { code: 'snapshot_changed', message: 'queue changed', retryable: true };
      const prior = view.value.intelligenceOperation;
      view.value = {
        ...view.value,
        mode,
        intelligence: null,
        intelligenceOperation: null,
        queueRevision: String(BigInt(revision) + 1n),
      };
      if (prior) {
        const item = attempts.find((a) => a.attempt.id === prior);
        if (item) {
          networkBusy.value = false;
          item.result.reject({ code: 'stale', message: 'left' });
        }
      }
      return view.value;
    },
  } as unknown as PlayerModel;
  const port: IntelligenceCycle = {
    eligible: () => eligible.value,
    pending: () => networkBusy.value,
    idle: (epoch, signal) => intelligenceIdle(networkBusy, player, epoch, signal),
    enter: (revision) => {
      const result = deferred<PlayerView>();
      const item = { attempt: null! as IntelligenceAttempt, result, revision, cancelled: 0 };
      item.attempt = {
        id: crypto.randomUUID(),
        epoch: view.value.epoch,
        promise: result.promise,
        cancelRequested: () => item.cancelled > 0,
        cancel: async () => {
          item.cancelled++;
          networkBusy.value = false;
          result.reject({ code: 'stale', message: 'cancelled' });
        },
      };
      attempts.push(item);
      networkBusy.value = true;
      return item.attempt;
    },
  };
  const scope = effectScope(),
    model = scope.run(() => usePlaybackMode(player, () => together.value, port))!;
  function seed(item = attempts.at(-1)!, phase: 'seeded' | 'active' | 'fallback' = 'seeded') {
    view.value = {
      ...view.value,
      mode: 'order_loop',
      intelligence: phase,
      intelligenceOperation: item.attempt.id,
      queueRevision: String(BigInt(view.value.queueRevision) + 1n),
      playId: 'seed-' + item.attempt.id,
    };
  }
  return {
    model,
    view,
    ready,
    networkBusy,
    transportBusy,
    eligible,
    together,
    modes,
    attempts,
    seed,
    snapshots: () => snapshots,
    stop: () => scope.stop(),
  };
}
test('all five-mode and together next targets match the executed official factory', () => {
  const frozen = JSON.parse(
    readFileSync(
      new URL('../../core/tests/fixtures/official-intelligence-modes.json', import.meta.url),
      'utf8',
    ),
  );
  assert.equal(frozen.vectors.length, 20);
  for (const row of frozen.vectors)
    assert.equal(
      nextModeTarget(row.current, row.together ? 'together' : 'ordinary', row.canHeartbeat),
      row.next,
    );
  let target: ModeTarget = 'intelligence';
  const observed: ModeTarget[] = [];
  for (let i = 0; i < 5; i++) {
    target = nextModeTarget(target, 'ordinary', true);
    observed.push(target);
  }
  assert.deepEqual(observed, ['order_loop', 'single_loop', 'random', 'sequential', 'intelligence']);
});
test('eligibility follows the actual liked-playlist origin and excludes FM or an unknown account list', () => {
  const f = fixture();
  assert.equal(canHeartBeat(f.view.value, '31'), true);
  for (const origin of [{ kind: 'album', id: '31' }, { kind: 'playlist', id: '32' }, null])
    assert.equal(canHeartBeat({ ...f.view.value, origin } as PlayerView, '31'), false);
  assert.equal(canHeartBeat({ ...f.view.value, fm: true }, '31'), false);
  assert.equal(canHeartBeat(f.view.value, null), false);
  f.stop();
});
test('a correlated seed releases mode controls while its recommendation HTTP remains pending', async () => {
  const f = fixture();
  f.model.cycle();
  assert.equal(f.model.target.value, 'intelligence');
  assert.equal(f.model.pending.value, true);
  assert.equal(f.transportBusy.value, false);
  f.seed();
  await turn();
  assert.equal(f.model.family.value, 'intelligence');
  assert.equal(f.model.pending.value, false);
  assert.equal(f.networkBusy.value, true);
  f.model.cycle();
  await turn();
  assert.deepEqual(f.modes, [{ mode: 'order_loop', revision: '2' }]);
  assert.equal(f.model.family.value, 'ordinary');
  f.stop();
});
test('five rapid clicks from an acknowledged AI seed exit once and enter once on the restored queue', async () => {
  const f = fixture();
  f.model.cycle();
  f.seed();
  await turn();
  for (let i = 0; i < 5; i++) f.model.cycle();
  await turn();
  assert.deepEqual(f.modes, [{ mode: 'order_loop', revision: '2' }]);
  assert.equal(f.attempts.length, 2);
  assert.equal(f.attempts[1]?.revision, '3');
  f.seed();
  await turn();
  assert.equal(f.model.family.value, 'intelligence');
  assert.equal(f.model.pending.value, false);
  f.stop();
  f.attempts[1]!.result.reject({ code: 'stale' });
});
test('an unavailable heart entry leaves the ordinary four-state ring intact', async () => {
  const f = fixture();
  f.eligible.value = false;
  f.model.cycle();
  await turn();
  assert.deepEqual(f.modes, [{ mode: 'order_loop', revision: '1' }]);
  assert.equal(f.attempts.length, 0);
  f.stop();
});
test('rapid clicks can cancel an uncommitted entry and apply the final ordinary target without a seed', async () => {
  const f = fixture();
  f.model.cycle();
  f.model.cycle();
  f.model.cycle();
  f.model.cycle();
  assert.equal(f.model.target.value, 'random');
  await turn();
  assert.equal(f.attempts.length, 1);
  assert.equal(f.attempts[0]!.cancelled, 1);
  assert.equal(f.snapshots(), 1);
  assert.deepEqual(f.modes, [{ mode: 'random', revision: '1' }]);
  assert.equal(f.model.issue.value, undefined);
  f.stop();
});
test('cancelling from the shared feedback keeps the original mode without inventing a mode error', async () => {
  const f = fixture();
  f.model.cycle();
  await f.attempts[0]!.attempt.cancel();
  await turn();
  assert.equal(f.model.issue.value, undefined);
  assert.equal(f.model.target.value, 'sequential');
  assert.equal(f.model.pending.value, false);
  assert.equal(f.modes.length, 0);
  assert.equal(f.snapshots(), 1);
  f.stop();
});
test('a committed seed that overtakes cancellation is exited with its fresh native revision', async () => {
  const f = fixture();
  f.model.cycle();
  f.model.cycle();
  f.seed();
  await turn();
  assert.deepEqual(f.modes, [{ mode: 'order_loop', revision: '2' }]);
  assert.equal(f.model.issue.value, undefined);
  assert.equal(f.snapshots(), 0);
  f.stop();
});
test('AI can be queued after an ordinary commit and skipped again before any entry request is sent', async () => {
  const held = deferred<void>(),
    f = fixture(() => held.promise);
  f.view.value = { ...f.view.value, mode: 'random' };
  f.model.cycle();
  f.model.cycle();
  f.model.cycle();
  assert.equal(f.model.target.value, 'order_loop');
  held.resolve();
  await turn();
  assert.equal(f.attempts.length, 0);
  assert.deepEqual(f.modes, [
    { mode: 'sequential', revision: '1' },
    { mode: 'order_loop', revision: '2' },
  ]);
  f.stop();
});
test('the last AI target after an ordinary burst starts only on the acknowledged queue', async () => {
  const held = deferred<void>(),
    f = fixture(() => held.promise);
  f.view.value = { ...f.view.value, mode: 'random' };
  f.model.cycle();
  f.model.cycle();
  held.resolve();
  await turn();
  assert.equal(f.attempts[0]?.revision, '2');
  f.seed();
  await turn();
  assert.equal(f.model.family.value, 'intelligence');
  f.stop();
  f.attempts[0]!.result.reject({ code: 'stale' });
});
test('a queued entry waits for the old feature read and honors a later ordinary click', async () => {
  const f = fixture();
  f.networkBusy.value = true;
  f.model.cycle();
  f.model.cycle();
  assert.equal(f.attempts.length, 0);
  f.networkBusy.value = false;
  await turn();
  assert.equal(f.attempts.length, 0);
  assert.deepEqual(f.modes, [{ mode: 'order_loop', revision: '1' }]);
  f.stop();
});
test('pre-seed failure keeps the actual mode and explicit retry receives a fresh operation and revision', async () => {
  const f = fixture();
  f.model.cycle();
  const first = f.attempts[0]!;
  f.networkBusy.value = false;
  first.result.reject({ code: 'storage', message: 'disk failed', retryable: true });
  await turn();
  assert.equal(f.model.target.value, 'sequential');
  assert.equal(f.model.canRetry.value, true);
  assert.equal(f.model.issue.value?.code, 'storage');
  f.view.value = { ...f.view.value, queueRevision: '3' };
  f.model.retry();
  assert.equal(f.attempts[1]?.revision, '3');
  assert.notEqual(f.attempts[1]?.attempt.id, first.attempt.id);
  f.seed();
  await turn();
  assert.equal(f.model.issue.value, undefined);
  f.stop();
  f.attempts[1]!.result.reject({ code: 'stale' });
});
test('another window cannot satisfy this entry receipt or receive its queued ordinary modes', async () => {
  const f = fixture();
  f.model.cycle();
  f.model.cycle();
  const request = f.attempts[0]!;
  f.view.value = {
    ...f.view.value,
    intelligence: 'seeded',
    intelligenceOperation: crypto.randomUUID(),
    mode: 'order_loop',
    queueRevision: '99',
    playId: 'other-window',
  };
  await turn();
  assert.equal(f.model.pending.value, false);
  assert.equal(f.model.issue.value, undefined);
  assert.equal(f.modes.length, 0);
  assert.ok(request.cancelled >= 1);
  f.stop();
});
test('a new account or source invalidates the pending entry and discards its late failure', async () => {
  for (const change of ['account', 'source']) {
    const f = fixture();
    f.model.cycle();
    const request = f.attempts[0]!;
    f.view.value = {
      ...f.view.value,
      epoch: change === 'account' ? '2' : '1',
      queueRevision: '99',
      playId: 'new-source',
      origin: { kind: 'album', id: '51' },
    };
    f.networkBusy.value = false;
    request.result.reject({ code: 'stale', message: 'old request' });
    await turn();
    assert.equal(f.model.issue.value, undefined);
    assert.equal(f.model.pending.value, false);
    assert.equal(f.modes.length, 0);
    f.stop();
  }
});
