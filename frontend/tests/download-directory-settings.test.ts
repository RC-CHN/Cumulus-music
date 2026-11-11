import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  createDownloadDirectorySettings,
  type DownloadDirectorySettingsOwner,
} from '../src/composables/useDownloadDirectorySettings.ts';
import type { DownloadDirectoryPreview, DownloadResult } from '../src/services/downloads.ts';

type Ports = Parameters<typeof createDownloadDirectorySettings>[0];
const preview: DownloadDirectoryPreview = {
  epoch: 'A',
  target: '/sealed/normalized-music',
  revision: 'original-revision',
  decision: 'ready',
  moveExisting: true,
};
const done = (operationId: string | null): DownloadResult => ({
  epoch: 'A',
  accepted: [],
  failed: [],
  operationId,
});

function fixture(overrides: Partial<Ports> = {}) {
  let owner: DownloadDirectorySettingsOwner | undefined = { epoch: 'A', visit: {} };
  const calls: unknown[][] = [];
  const ports: Ports = {
    owner: () => owner,
    blocked: () => false,
    pick: async (epoch) => {
      calls.push(['pick', epoch]);
      return '/sealed/picked-music';
    },
    preview: async (epoch, path) => {
      calls.push(['preview', epoch, path]);
      return preview;
    },
    commit: async (epoch, command) => {
      calls.push(['commit', epoch, command]);
      return done('migration-1');
    },
    expired: () => calls.push(['expired']),
    ...overrides,
  };
  const model = createDownloadDirectorySettings(ports);
  return {
    model,
    calls,
    change(value: DownloadDirectorySettingsOwner | undefined) {
      owner = value;
      model.retire();
    },
  };
}

test('cancelled native picker opens at the first action, stays single, and sends no preview', async () => {
  const pending = Promise.withResolvers<string | null>();
  let picks = 0;
  const { model, calls } = fixture({
    pick: (epoch) => {
      assert.equal(epoch, 'A');
      picks++;
      return pending.promise;
    },
  });
  const choosing = model.choose();
  assert.equal(model.activity.value, 'picking');
  await model.choose();
  assert.equal(picks, 1);
  pending.resolve(null);
  await choosing;
  assert.equal(model.activity.value, undefined);
  assert.equal(model.confirmation.value, undefined);
  assert.deepEqual(calls, []);
});

test('directory preview remains read-only and confirmation uses its normalized path and revision once', async () => {
  const pending = Promise.withResolvers<DownloadResult>();
  const writes: unknown[][] = [];
  const { model, calls } = fixture({
    commit: (epoch, command) => {
      writes.push([epoch, command]);
      return pending.promise;
    },
  });
  await model.choose();
  assert.deepEqual(calls, [
    ['pick', 'A'],
    ['preview', 'A', '/sealed/picked-music'],
  ]);
  assert.equal(model.confirmation.value?.preview, preview);
  assert.deepEqual(writes, []);
  const saving = model.confirm();
  await model.confirm();
  assert.deepEqual(writes, [
    ['A', { kind: 'change_directory', root: preview.target, revision: preview.revision }],
  ]);
  pending.resolve(done('migration-1'));
  await saving;
  assert.equal(model.confirmation.value, undefined);
  assert.match(model.feedback.value?.text ?? '', /迁移已开始/);
});

test('each rejected native preview waits for an explicit repick and never submits', async () => {
  for (const decision of ['same_directory', 'not_writable', 'insufficient_space'] as const) {
    let previews = 0;
    const { model, calls } = fixture({
      preview: async () => ({ ...preview, decision: ++previews === 1 ? decision : 'ready' }),
    });
    await model.choose();
    assert.equal(model.confirmation.value?.preview.decision, decision);
    await model.confirm();
    assert.equal(previews, 1);
    assert.deepEqual(calls, [['pick', 'A']]);
    model.dismiss();
    await model.choose();
    assert.equal(previews, 2);
    assert.equal(model.confirmation.value?.preview.decision, 'ready');
    model.dismiss();
    assert.equal(model.confirmation.value, undefined);
    assert.deepEqual(calls, [
      ['pick', 'A'],
      ['pick', 'A'],
    ]);
  }
});

test('route and account changes retire late picker and preview replies', async () => {
  const picked = Promise.withResolvers<string | null>();
  const picker = fixture({ pick: () => picked.promise });
  const first = picker.model.choose();
  picker.change({ epoch: 'A', visit: {} });
  picked.resolve('/sealed/late');
  await first;
  assert.deepEqual(picker.calls, []);
  assert.equal(picker.model.confirmation.value, undefined);

  const inspected = Promise.withResolvers<DownloadDirectoryPreview>();
  const probe = fixture({ preview: () => inspected.promise });
  const second = probe.model.choose();
  await Promise.resolve();
  assert.equal(probe.model.activity.value, 'previewing');
  probe.change({ epoch: 'B', visit: {} });
  inspected.resolve(preview);
  await second;
  assert.equal(probe.model.confirmation.value, undefined);
  assert.equal(probe.model.activity.value, undefined);
});

test('unknown commit closes confirmation, asks for readback and cannot replay', async () => {
  let attempts = 0;
  const { model, calls } = fixture({
    commit: async () => {
      attempts++;
      throw { code: 'transport', message: 'synthetic lost acknowledgement' };
    },
  });
  await model.choose();
  await model.confirm();
  await model.confirm();
  assert.equal(attempts, 1);
  assert.equal(model.confirmation.value, undefined);
  assert.equal(model.feedback.value?.recheck, true);
  await model.choose();
  assert.deepEqual(calls, [
    ['pick', 'A'],
    ['preview', 'A', '/sealed/picked-music'],
  ]);
});

test('an old account completion does not paint success on the next settings visit', async () => {
  const pending = Promise.withResolvers<DownloadResult>();
  const { model, change } = fixture({ commit: () => pending.promise });
  await model.choose();
  const saving = model.confirm();
  change({ epoch: 'B', visit: {} });
  pending.resolve(done(null));
  await saving;
  assert.equal(model.feedback.value, undefined);
  assert.equal(model.confirmation.value, undefined);
});
