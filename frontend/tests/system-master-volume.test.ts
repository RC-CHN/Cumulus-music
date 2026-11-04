import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSystemMasterVolume } from '../src/composables/useSystemMasterVolume.ts';
import type { SystemMasterVolume } from '../src/services/systemMasterVolume.ts';

test('only a native supported result exposes diagnostics, while a confirmed zero is a number', async () => {
  let result: SystemMasterVolume = { kind: 'unsupported' };
  const model = useSystemMasterVolume({ systemMasterVolume: async () => result });
  assert.equal(model.visible.value, false);
  await model.read();
  assert.equal(model.visible.value, false);
  result = { kind: 'ready', percent: 0 };
  await model.read();
  assert.equal(model.visible.value, true);
  assert.deepEqual(model.view.value, { kind: 'ready', percent: 0 });
});

test('a failed refresh discards the old displayed number and can be explicitly retried', async () => {
  let failed = false;
  const model = useSystemMasterVolume({
    systemMasterVolume: async () => {
      if (failed) throw Error('synthetic host failure');
      return { kind: 'ready', percent: 37 };
    },
  });
  await model.read();
  failed = true;
  await model.read();
  assert.deepEqual(model.view.value, { kind: 'unavailable' });
  assert.equal(model.visible.value, true);
  failed = false;
  await model.read();
  assert.deepEqual(model.view.value, { kind: 'ready', percent: 37 });
});

test('only one read per view may run and a detached page rejects its late response', async () => {
  let finish!: (value: SystemMasterVolume) => void;
  const pending = new Promise<SystemMasterVolume>((resolve) => (finish = resolve));
  let calls = 0;
  const model = useSystemMasterVolume({
    systemMasterVolume: () => {
      calls++;
      return pending;
    },
  });
  const started = model.read();
  assert.equal(model.loading.value, true);
  await model.read();
  model.dispose();
  finish({ kind: 'ready', percent: 84 });
  await started;
  await model.read();
  assert.equal(model.visible.value, false);
  assert.equal(calls, 1);
});
