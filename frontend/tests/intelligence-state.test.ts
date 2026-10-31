import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, ref, shallowRef } from 'vue';
import { useIntelligence } from '../src/composables/useIntelligence.ts';
import type { PlayerModel } from '../src/composables/usePlayer.ts';
import type { CoreClient } from '../src/services/contracts.ts';
import type { IntelligenceCommand, PlayerView } from '../src/services/player.ts';
const turn = () => new Promise((r) => setImmediate(r));
function deferred() {
  let resolve!: () => void, reject!: (e: unknown) => void;
  const promise = new Promise<void>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const held = deferred(),
    view = shallowRef({ epoch: '1', queueRevision: '4', intelligence: null } as PlayerView),
    ready = ref(true),
    transportBusy = ref(false),
    together = ref(false);
  const calls: IntelligenceCommand[] = [],
    cancelled: { epoch: string; id: string }[] = [];
  const player = {
    view,
    ready,
    busy: transportBusy,
    intelligence: async (command: IntelligenceCommand) => {
      calls.push(command);
      await held.promise;
      return view.value;
    },
  } as unknown as PlayerModel;
  const core = {
    playerIntelligenceCancel: async (epoch: string, id: string) => {
      cancelled.push({ epoch, id });
      return true;
    },
  } as unknown as CoreClient;
  const scope = effectScope(),
    model = scope.run(() => useIntelligence(player, () => together.value, core))!;
  return {
    held,
    view,
    ready,
    transportBusy,
    together,
    calls,
    cancelled,
    model,
    stop: () => scope.stop(),
  };
}
test('start owns its progress and cancel identity without blocking transport or duplicate requests', async () => {
  const f = fixture();
  const request = f.model.start({ kind: 'likes', playlist_id: '31', revision: 'liked-revision' });
  assert.equal(f.model.busy.value, true);
  assert.equal(f.transportBusy.value, false);
  await f.model.start({ kind: 'current' });
  assert.equal(f.calls.length, 1);
  const command = f.calls[0]!;
  assert.equal(command.expected_queue_revision, '4');
  assert.equal(command.kind, 'start');
  await f.model.cancel();
  assert.deepEqual(f.cancelled, [{ epoch: '1', id: command.operation_id }]);
  assert.equal(f.model.cancelling.value, true);
  f.held.reject({ code: 'stale', message: 'cancelled' });
  await request;
  assert.equal(f.model.busy.value, false);
  assert.equal(f.model.issue.value, undefined);
  f.stop();
});
test('persisted seeded phase requires explicit continuation and never auto-fetches or replays a seed', async () => {
  const f = fixture();
  f.view.value = { ...f.view.value, intelligence: 'seeded' };
  await turn();
  assert.equal(f.model.resumable.value, true);
  assert.equal(f.calls.length, 0);
  assert.equal(f.model.canStart.value, false);
  const request = f.model.resume();
  assert.equal(f.calls[0]?.kind, 'continue');
  assert.equal(f.model.resumable.value, false);
  f.held.resolve();
  await request;
  f.stop();
});
test('account or connection replacement cancels only its old operation and discards late failures', async () => {
  const f = fixture();
  const request = f.model.start({ kind: 'current' });
  f.view.value = { ...f.view.value, epoch: '2' };
  await turn();
  assert.equal(f.cancelled[0]?.epoch, '1');
  assert.equal(f.model.busy.value, false);
  f.held.reject({ code: 'transport', message: 'late failure' });
  await request;
  assert.equal(f.model.issue.value, undefined);
  f.stop();
});
test('together disables start and continuation while fallback is ordinary and can be explicitly restarted', async () => {
  const f = fixture();
  f.together.value = true;
  await f.model.start({ kind: 'current' });
  assert.equal(f.calls.length, 0);
  f.view.value = { ...f.view.value, intelligence: 'seeded' };
  assert.equal(f.model.resumable.value, false);
  f.together.value = false;
  assert.equal(f.model.resumable.value, true);
  f.view.value = { ...f.view.value, intelligence: 'fallback' };
  assert.equal(f.model.active.value, false);
  assert.equal(f.model.canStart.value, true);
  f.stop();
});
test('action failures remain local and can be dismissed without falling into audio retry', async () => {
  const f = fixture();
  const request = f.model.start({ kind: 'current' });
  f.held.reject({ code: 'snapshot_changed', message: '列表已更新', retryable: true });
  await request;
  assert.equal(f.model.issue.value?.code, 'snapshot_changed');
  assert.equal(f.transportBusy.value, false);
  f.model.dismiss();
  assert.equal(f.model.issue.value, undefined);
  f.stop();
});
test('a native fallback result produces explicit failure feedback and permits a fresh start', async () => {
  const f = fixture();
  const request = f.model.start({ kind: 'likes', playlist_id: '31' });
  f.view.value = { ...f.view.value, intelligence: 'fallback' };
  f.held.resolve();
  await request;
  assert.equal(f.model.issue.value?.code, 'intelligence_failed');
  assert.equal(f.model.canStart.value, true);
  f.stop();
});
