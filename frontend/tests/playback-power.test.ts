import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlaybackPower } from '../src/composables/usePlaybackPower.ts';
import type { PlaybackPowerClient, PlaybackPowerView } from '../src/services/playbackPower.ts';

const view = (enabled = false, available = true): PlaybackPowerView => ({ available, enabled });
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function backend(extra: Partial<PlaybackPowerClient> = {}): PlaybackPowerClient {
  return {
    playbackPower: async () => view(),
    setPlaybackPower: async (_, enabled) => view(enabled),
    ...extra,
  };
}

test('native capability gates visibility and any write even when the hidden preference is on', async () => {
  const pending = deferred<PlaybackPowerView>();
  let writes = 0;
  const model = usePlaybackPower(
    backend({
      playbackPower: () => pending.promise,
      setPlaybackPower: async (_, enabled) => {
        writes++;
        return view(enabled);
      },
    }),
  );
  const initial = model.connect('1');
  assert.equal(model.visible.value, false);
  assert.equal(await model.change(true), false);
  pending.resolve(view(true, false));
  await initial;
  assert.equal(model.visible.value, false);
  assert.equal(model.canChange.value, false);
  assert.equal(await model.change(false), false);
  assert.equal(writes, 0);
});

test('saving waits for native acknowledgement, does not repeat, and rejects mismatched readback', async () => {
  const receipt = deferred<PlaybackPowerView>();
  const calls: [string, boolean][] = [];
  const model = usePlaybackPower(
    backend({
      setPlaybackPower: (owner, enabled) => {
        calls.push([owner, enabled]);
        return receipt.promise;
      },
    }),
  );
  await model.connect('1');
  const saving = model.change(true);
  assert.equal(model.busy.value, true);
  assert.equal(model.view.value?.enabled, false);
  assert.equal(model.saved.value, false);
  assert.equal(await model.change(true), false);
  receipt.resolve(view(false));
  assert.equal(await saving, false);
  assert.equal(model.saved.value, false);
  assert.equal(model.view.value?.enabled, false);
  assert.ok(model.error.value);
  assert.deepEqual(calls, [['1', true]]);
});

test('a lost write acknowledgement re-reads once without replay and offers explicit recovery', async () => {
  let stored = view(),
    reads = 0,
    writes = 0;
  const model = usePlaybackPower(
    backend({
      playbackPower: async () => {
        reads++;
        return stored;
      },
      setPlaybackPower: async (_, enabled) => {
        writes++;
        stored = view(enabled);
        if (writes === 1) throw { code: 'storage', message: '保存回执丢失' };
        return stored;
      },
    }),
  );
  await model.connect('1');
  assert.equal(await model.change(true), false);
  assert.equal(model.view.value?.enabled, true);
  assert.equal(model.saved.value, false);
  assert.equal(model.error.value, '保存回执丢失');
  assert.equal(await model.change(false), false);
  assert.deepEqual([reads, writes], [2, 1]);
  await model.read();
  assert.equal(model.canChange.value, true);
  assert.equal(await model.change(false), true);
  assert.equal(model.saved.value, true);
  assert.equal(model.view.value?.enabled, false);
  assert.deepEqual([reads, writes], [3, 2]);
});

test('late reads and writes from another account or departed page are ignored', async () => {
  const oldRead = deferred<PlaybackPowerView>();
  const oldWrite = deferred<PlaybackPowerView>();
  const reads: string[] = [];
  const model = usePlaybackPower(
    backend({
      playbackPower: (owner) => {
        reads.push(owner);
        return owner === '1' ? oldRead.promise : Promise.resolve(view(false, owner !== '3'));
      },
      setPlaybackPower: () => oldWrite.promise,
    }),
  );
  const reading = model.connect('1');
  await model.connect('2');
  oldRead.resolve(view(true));
  await reading;
  assert.equal(model.view.value?.enabled, false);
  const writing = model.change(true);
  await model.connect('3');
  oldWrite.reject({ code: 'stale', message: '旧账号' });
  assert.equal(await writing, false);
  assert.equal(model.visible.value, false);
  assert.equal(model.error.value, '');
  assert.deepEqual(reads, ['1', '2', '3']);

  const pending = deferred<PlaybackPowerView>();
  const other = usePlaybackPower(backend({ setPlaybackPower: () => pending.promise }));
  await other.connect('1');
  const departing = other.change(true);
  other.dispose();
  pending.resolve(view(true));
  assert.equal(await departing, false);
  assert.equal(other.view.value?.enabled, false);
});

test('revoked native capability hides the checkbox without reporting that a write succeeded', async () => {
  const model = usePlaybackPower(backend({ setPlaybackPower: async () => view(true, false) }));
  await model.connect('1');
  assert.equal(await model.change(true), false);
  assert.equal(model.visible.value, false);
  assert.equal(model.canChange.value, false);
  assert.equal(model.saved.value, false);
});
