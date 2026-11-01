import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope } from 'vue';
import { useCloud } from '../src/composables/useCloud.ts';
import { useCloudDelete } from '../src/composables/useCloudDelete.ts';
import type { CloudClient, CloudDelete, CloudPage, CloudResult } from '../src/services/cloud.ts';
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
async function fixture() {
  let records: CloudDelete[] = [],
    revision = 'r1',
    ended = 0;
  const calls: string[] = [];
  let write: () => Promise<CloudResult> = async () => ({
    epoch: '1',
    accepted: [],
    failed: [],
    deletion: { id: 'ours', ids: ['7'], phase: 'rejected', total: 1, removed: 0, remaining: 1 },
  });
  const model = useCloud({
    cloudPage: async (e) =>
      ({
        epoch: e,
        revision,
        updatedMs: 1,
        size: 1,
        maxSize: 100,
        uploadedCount: 1,
        uploadingCount: 0,
        total: 1,
        offset: 0,
        tracks: [],
        uploads: [],
        deletions: records,
        workerError: null,
      }) satisfies CloudPage,
    cloudCommand: async (_e, input) => {
      calls.push(input.kind);
      return write();
    },
  } as CloudClient);
  await model.connect({ epoch: '1', status: 'authenticated', nickname: null, avatarUrl: null });
  await model.refresh();
  const scope = effectScope(),
    state = scope.run(() =>
      useCloudDelete(
        model,
        { epoch: '1', ids: ['7'], titles: ['原歌曲'], revision: 'r1' },
        () => ended++,
      ),
    )!;
  return {
    model,
    state,
    scope,
    calls,
    get ended() {
      return ended;
    },
    records(v: CloudDelete[]) {
      records = v;
    },
    revision(v: string) {
      revision = v;
    },
    write(v: () => Promise<CloudResult>) {
      write = v;
    },
  };
}
test('rejected deletion remains reviewable and never repeats until a fresh explicit confirmation', async () => {
  const f = await fixture();
  await f.state.send();
  assert.equal(f.state.operation.value?.phase, 'rejected');
  assert.equal(f.ended, 0);
  assert.equal(f.state.canSend.value, false);
  await f.state.send();
  await f.state.read();
  assert.deepEqual(f.calls, ['delete']);
  f.scope.stop();
  f.model.dispose();
});
test('lost delete acknowledgement recovers only related journal targets and reconciles without another delete', async () => {
  const f = await fixture();
  const ours: CloudDelete = {
    id: 'ours',
    ids: ['7'],
    phase: 'uncertain',
    total: 1,
    removed: 0,
    remaining: 1,
  };
  f.write(async () => {
    f.records([{ ...ours, id: 'other', ids: ['99'], phase: 'confirmed' }, ours]);
    throw { code: 'transport', message: 'lost IPC' };
  });
  await f.state.send();
  assert.equal(f.state.operation.value?.id, 'ours');
  assert.equal(f.state.canSend.value, false);
  await f.state.send();
  f.write(async () => ({
    epoch: '1',
    accepted: [],
    failed: [],
    deletion: { ...ours, phase: 'confirmed', removed: 1, remaining: 0 },
  }));
  await f.state.read();
  assert.equal(f.ended, 1);
  assert.deepEqual(f.calls, ['delete', 'reconcile_delete']);
  f.scope.stop();
  f.model.dispose();
});
test('no journal record after a lost reply cannot unlock a repeated delete, and a changed snapshot disables the old confirmation', async () => {
  const f = await fixture();
  f.write(async () => {
    throw { code: 'transport', message: 'lost' };
  });
  await f.state.send();
  assert.equal(f.state.unknown.value, true);
  await f.state.read();
  await f.state.send();
  assert.deepEqual(f.calls, ['delete']);
  assert.equal(f.state.canSend.value, false);
  f.scope.stop();
  f.model.dispose();
  const g = await fixture();
  g.revision('r2');
  await g.model.refresh();
  assert.equal(g.state.canSend.value, false);
  await g.state.send();
  assert.deepEqual(g.calls, []);
  g.scope.stop();
  g.model.dispose();
});
test('account replacement or unmount cannot receive a late deletion close or dispatch another action', async () => {
  const f = await fixture(),
    held = deferred<CloudResult>();
  f.write(() => held.promise);
  const sending = f.state.send();
  await f.model.connect({ epoch: '2', status: 'authenticated', nickname: null, avatarUrl: null });
  held.resolve({
    epoch: '1',
    accepted: [],
    failed: [],
    deletion: { id: 'ours', ids: ['7'], phase: 'confirmed', total: 1, removed: 1, remaining: 0 },
  });
  await sending;
  assert.equal(f.ended, 0);
  assert.equal(f.model.notice.value, '');
  await f.state.send();
  assert.deepEqual(f.calls, ['delete']);
  f.scope.stop();
  f.model.dispose();
  const g = await fixture();
  g.scope.stop();
  await g.state.send();
  await g.state.read();
  assert.deepEqual(g.calls, []);
  g.model.dispose();
});
