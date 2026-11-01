import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { CloudRemoval } from '../src/services/cloud.ts';
import { fixture, deferred } from './cloud-delete-fixture.ts';
test('preview precedes deletion; rejection freezes the original local choice and never resends', async () => {
  const f = await fixture();
  assert.equal(f.state.canSend.value, false);
  await f.state.prepare();
  assert.equal(f.state.canSend.value, true);
  await f.state.send();
  assert.equal(f.state.operation.value?.phase, 'rejected');
  assert.equal(f.ended, 0);
  f.state.deleteLocal.value = true;
  await f.state.send();
  await f.state.read();
  await f.state.resume();
  assert.deepEqual(f.calls, [
    ['prepare', '1', ['7'], 'r1'],
    ['execute', '1', 'ours', false, false],
  ]);
  f.dispose();
});
test('lost acknowledgement reads the exact accepted plan and never repeats a remote mutation', async () => {
  const f = await fixture();
  await f.state.prepare();
  f.execute(async () => {
    throw { code: 'transport', message: 'lost IPC' };
  });
  await f.state.send();
  assert(f.state.unknown.value);
  await f.state.send();
  f.receipt({
    phase: 'result',
    deleteLocal: false,
    files: [],
    deletion: { id: 'ours', ids: ['7'], phase: 'confirmed', total: 1, removed: 1, remaining: 0 },
  });
  await f.state.read();
  assert.equal(f.ended, 1);
  assert.deepEqual(
    f.calls.map((x) => x[0]),
    ['prepare', 'execute', 'read'],
  );
  assert.deepEqual(f.calls[2], ['read', '1', 'ours', true]);
  f.dispose();
});
test('a still-unaccepted read after a lost reply stays read-only and never discards the plan', async () => {
  const f = await fixture();
  await f.state.prepare();
  f.execute(async () => {
    throw { code: 'transport', message: 'lost' };
  });
  await f.state.send();
  await f.state.read();
  await f.state.send();
  assert(f.state.unknown.value);
  assert(!f.state.canSend.value);
  f.dispose();
  assert.deepEqual(
    f.calls.map((x) => x[0]),
    ['prepare', 'execute', 'read'],
  );
});
test('prepared revisions and account replacement fence stale writes and late closes', async () => {
  const f = await fixture();
  await f.state.prepare();
  f.revision('r2');
  await f.model.refresh();
  assert(!f.state.canSend.value);
  await f.state.send();
  assert.equal(f.calls.length, 1);
  f.dispose();
  const g = await fixture();
  await g.state.prepare();
  const held = deferred<CloudRemoval>();
  g.execute(() => held.promise);
  const sending = g.state.send();
  await g.model.connect({ epoch: '2', status: 'authenticated', nickname: null, avatarUrl: null });
  held.resolve({} as CloudRemoval);
  await sending;
  assert.equal(g.ended, 0);
  assert.equal(g.model.notice.value, '');
  await g.state.send();
  assert.equal(g.calls.filter((x) => x[0] === 'execute').length, 1);
  g.dispose();
});
test('recovered accepted plans replace the displayed scope and freeze the previously confirmed checkbox', async () => {
  const f = await fixture();
  f.receipt({
    ids: ['7', '8'],
    titles: ['Original', 'Other original'],
    kinds: ['private', 'ordinary'],
    phase: 'local',
    deleteLocal: true,
    canContinue: true,
  });
  await f.state.prepare();
  assert.deepEqual(f.state.target.ids, ['7', '8']);
  assert(f.state.deleteLocal.value);
  assert(!f.state.canSend.value);
  assert(f.state.canContinue.value);
  await f.state.resume();
  assert.deepEqual(f.calls[1], ['execute', '1', 'ours', true, true]);
  f.dispose();
});
test('closing a pending preview discards only the returned unconfirmed plan', async () => {
  const f = await fixture();
  const held = deferred<CloudRemoval>();
  f.api.cloudDeletePrepare = () => held.promise;
  const loading = f.state.prepare();
  f.scope.stop();
  held.resolve({ epoch: '1', id: 'late-preview', deleteLocal: null } as CloudRemoval);
  await loading;
  assert.deepEqual(f.calls, [['discard', '1', 'late-preview']]);
  f.model.dispose();
});
