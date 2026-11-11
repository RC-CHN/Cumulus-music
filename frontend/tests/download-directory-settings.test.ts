import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  createDownloadDirectorySettings,
  displayDownloadDirectory,
  type DownloadDirectorySettingsOwner,
} from '../src/composables/useDownloadDirectorySettings.ts';
import type { DownloadCommandObservation } from '../src/composables/useDownloads.ts';
import type {
  DownloadDirectoryPreview,
  DownloadResult,
  DownloadsView,
} from '../src/services/downloads.ts';

test('display removes the Windows verbatim prefix only for drive and share paths', () => {
  const slash = '\\';
  const prefix = slash.repeat(2) + '?' + slash;
  const local = ['D:', 'Music', 'Album'].join(slash);
  const share = ['server', 'Music', 'Album'].join(slash);
  const volume = prefix + 'Volume{sample}' + slash + 'Music';
  assert.equal(displayDownloadDirectory(prefix + local), local);
  assert.equal(displayDownloadDirectory(prefix + 'UNC' + slash + share), slash.repeat(2) + share);
  assert.equal(displayDownloadDirectory(volume), volume);
  assert.equal(displayDownloadDirectory('/home/listener/Music'), '/home/listener/Music');
});

type Ports = Parameters<typeof createDownloadDirectorySettings>[0];
const preview: DownloadDirectoryPreview = {
  epoch: 'A',
  target: '/sealed/normalized-music',
  revision: 'original-revision',
  decision: 'ready',
  moveExisting: true,
  receipt: '0123456789abcdef'.repeat(2),
};
const done = (operationId: string | null): DownloadResult => ({
  epoch: 'A',
  accepted: [],
  failed: [],
  operationId,
});
const readback = (directory: DownloadsView['directory'] = null, epoch = 'A'): DownloadsView => ({
  epoch,
  tasks: [],
  total: 0,
  activeCount: 0,
  completedCount: 0,
  offset: 0,
  directory,
  options: { quality: 'standard', tags: false, cover: false, lyrics: false, container: 'audio' },
  migration: null,
  operations: [],
  jobs: [],
});
const moved = (id: string): DownloadCommandObservation => ({
  result: done(id),
  readback: {
    ...readback(),
    migration: {
      id,
      source: '/sealed/old-music',
      target: preview.target,
      phase: 'moving',
      items: [],
      skipped: [],
      waiting: [],
      required_bytes: 0,
      error: null,
    },
  },
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
    refresh: async () => readback(),
    commit: async (epoch, command) => {
      calls.push(['commit', epoch, command]);
      return moved('migration-1');
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

test('directory confirmation uses the exact preview path, revision and receipt once', async () => {
  const pending = Promise.withResolvers<DownloadCommandObservation>();
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
  assert.deepEqual(model.confirmation.value?.preview, preview);
  assert.deepEqual(writes, []);
  const saving = model.confirm();
  await model.confirm();
  assert.deepEqual(writes, [
    [
      'A',
      {
        kind: 'change_directory',
        root: preview.target,
        revision: preview.revision,
        receipt: preview.receipt,
      },
    ],
  ]);
  pending.resolve(moved('migration-1'));
  await saving;
  assert.equal(model.confirmation.value, undefined);
  assert.match(model.feedback.value?.text ?? '', /迁移已开始/);
});

test('missing or malformed native receipt cannot open a directory write confirmation', async () => {
  const { receipt: _, ...missing } = preview;
  const invalid = [
    missing,
    ...[null, 'a'.repeat(31), 'A'.repeat(32), 'a'.repeat(32) + '\n', { id: preview.receipt }].map(
      (receipt) => ({ ...preview, receipt }),
    ),
    { ...preview, decision: 'same_directory' },
    { ...preview, decision: 'unknown', receipt: null },
  ];
  for (const supplied of invalid) {
    const { model, calls } = fixture({ preview: async () => supplied as DownloadDirectoryPreview });
    await model.choose();
    assert.equal(model.confirmation.value, undefined);
    await model.confirm();
    assert.match(model.feedback.value?.text ?? '', /结果无效/);
    assert.deepEqual(calls, [['pick', 'A']]);
  }
});

test('each rejected native preview waits for an explicit repick and never submits', async () => {
  for (const decision of ['same_directory', 'not_writable', 'insufficient_space'] as const) {
    let previews = 0;
    const { model, calls } = fixture({
      preview: async () => (++previews === 1 ? { ...preview, decision, receipt: null } : preview),
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
  assert.match(model.feedback.value?.text ?? '', /lost acknowledgement/);
  await model.choose();
  assert.deepEqual(calls, [
    ['pick', 'A'],
    ['preview', 'A', '/sealed/picked-music'],
  ]);
});

test('a command acknowledgement without a matching readback does not claim success', async () => {
  const saved = { root: preview.target, revision: 'new', path_type: 0, file_type: 1 };
  const snapshots = [undefined, readback(saved, 'B'), readback(saved)];
  const { model, calls } = fixture({
    commit: async () => ({ result: done(null) }),
    refresh: async () => snapshots.shift(),
  });
  await model.choose();
  await model.confirm();
  assert.match(model.feedback.value?.text ?? '', /已提交，请核对/);
  assert.equal(model.feedback.value?.recheck, true);
  for (let i = 0; i < 2; i++) {
    await model.recheck();
    assert.match(model.feedback.value?.text ?? '', /未能读取/);
    assert.equal(model.feedback.value?.recheck, true);
    await model.choose();
  }
  assert.deepEqual(calls, [
    ['pick', 'A'],
    ['preview', 'A', '/sealed/picked-music'],
  ]);
  await model.recheck();
  assert.deepEqual(model.feedback.value, { text: '已读取当前下载目录。' });
});

test('an accepted same-account readback distinguishes stored path from command acknowledgement', async () => {
  const saved = { root: preview.target, revision: 'new', path_type: 0, file_type: 1 };
  for (const [snapshot, confirmed] of [
    [readback({ ...saved, root: '/sealed/another-folder' }), false],
    [readback({ ...saved, revision: preview.revision! }), false],
    [readback(saved), true],
  ] as const) {
    const { model } = fixture({ commit: async () => ({ result: done(null), readback: snapshot }) });
    await model.choose();
    await model.confirm();
    if (confirmed) assert.deepEqual(model.feedback.value, { text: '下载目录已更改。' });
    else assert.equal(model.feedback.value?.recheck, true);
  }
});

test('a known native rejection remains visible and a late readback cannot update another visit', async () => {
  const pending = Promise.withResolvers<DownloadsView>();
  const { model, change } = fixture({
    commit: async () => ({ failure: { code: 'stale', message: '', retryable: false } }),
    refresh: () => pending.promise,
  });
  await model.choose();
  await model.confirm();
  assert.match(model.feedback.value?.text ?? '', /选择已失效/);
  assert.equal(model.feedback.value?.recheck, true);
  const checking = model.recheck();
  assert.equal(model.activity.value, 'checking');
  change({ epoch: 'A', visit: {} });
  pending.resolve(readback());
  await checking;
  assert.equal(model.feedback.value, undefined);
  assert.equal(model.activity.value, undefined);
});

test('an old account completion does not paint success on the next settings visit', async () => {
  const pending = Promise.withResolvers<DownloadCommandObservation>();
  const { model, change } = fixture({ commit: () => pending.promise });
  await model.choose();
  const saving = model.confirm();
  change({ epoch: 'B', visit: {} });
  pending.resolve({ result: done(null), readback: readback() });
  await saving;
  assert.equal(model.feedback.value, undefined);
  assert.equal(model.confirmation.value, undefined);
});
