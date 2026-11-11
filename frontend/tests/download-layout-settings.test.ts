import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createDownloadLayoutSettings } from '../src/composables/useDownloadLayoutSettings.ts';
import type { DownloadLayoutSettings } from '../src/services/downloads.ts';

type Ports = Parameters<typeof createDownloadLayoutSettings>[0];
const defaults: DownloadLayoutSettings = { fileType: 0, pathType: 0, revision: null };

function fixture(overrides: Partial<Ports> = {}) {
  let owner: ReturnType<Ports['owner']> = { epoch: 'A', visit: {} };
  let stored = defaults;
  const calls: unknown[][] = [];
  const layout = createDownloadLayoutSettings({
    owner: () => owner,
    blocked: () => false,
    read: async (epoch) => {
      calls.push(['read', epoch]);
      return stored;
    },
    change: async (epoch, change) => {
      calls.push(['change', epoch, change]);
      stored = {
        ...stored,
        [change.field === 'file_type' ? 'fileType' : 'pathType']: change.value,
        revision: 'stored',
      };
      return stored;
    },
    directoryChanged: () => calls.push(['refresh-directory']),
    expired: () => calls.push(['expired']),
    ...overrides,
  });
  return {
    layout,
    calls,
    visit(epoch: string) {
      owner = { epoch, visit: {} };
      layout.retire();
    },
  };
}

test('one changed radio saves only that field immediately, and the native readback sets the next revision', async () => {
  const { layout, calls } = fixture();
  await layout.refresh();
  assert.equal(layout.shown('file_type'), 0);
  assert.equal(layout.shown('path_type'), 0);
  await layout.choose('path_type', 0);
  assert.deepEqual(calls, [['read', 'A']]);

  const saving = layout.choose('file_type', 2);
  assert.equal(layout.shown('file_type'), 2);
  assert.equal(layout.shown('path_type'), 0);
  await layout.choose('path_type', 1); // Do not submit a second field with the old revision.
  await saving;
  assert.equal(layout.value.value?.revision, 'stored');
  await layout.choose('path_type', 1);
  assert.deepEqual(calls, [
    ['read', 'A'],
    ['change', 'A', { field: 'file_type', value: 2, expectedRevision: null }],
    ['read', 'A'],
    ['refresh-directory'],
    ['change', 'A', { field: 'path_type', value: 1, expectedRevision: 'stored' }],
    ['read', 'A'],
    ['refresh-directory'],
  ]);
  assert.equal(layout.shown('file_type'), 2);
  assert.equal(layout.shown('path_type'), 1);
  assert.equal(layout.feedback.value, undefined);
});

test('rejected writes read the authoritative layout without retrying the write', async () => {
  const observed: DownloadLayoutSettings = { fileType: 1, pathType: 2, revision: 'external' };
  let readCount = 0;
  let writes = 0;
  const { layout } = fixture({
    read: async () => (++readCount === 1 ? defaults : observed),
    change: async () => {
      writes++;
      throw { code: 'stale', message: 'different revision' };
    },
  });
  await layout.refresh();
  await layout.choose('file_type', 2);
  assert.equal(layout.value.value, observed);
  assert.equal(layout.shown('file_type'), 1);
  assert.equal(layout.shown('path_type'), 2);
  assert.match(layout.feedback.value?.text ?? '', /已变化.*当前设置/);
  assert.equal(readCount, 2);
  assert.equal(writes, 1);
});

test('an acknowledgement does not substitute for a failed readback; manual recheck sends no write', async () => {
  const saved: DownloadLayoutSettings = { fileType: 2, pathType: 0, revision: 'new' };
  let reads = 0;
  let writes = 0;
  const { layout } = fixture({
    read: async () => {
      reads++;
      if (reads === 2) throw new Error('disconnected');
      return reads === 1 ? defaults : saved;
    },
    change: async () => {
      writes++;
      return saved;
    },
  });
  await layout.refresh();
  await layout.choose('file_type', 2);
  assert.equal(layout.shown('file_type'), 0);
  assert.equal(layout.feedback.value?.recheck, true);
  assert.equal(layout.canChoose(), false);
  await layout.choose('path_type', 2);
  await layout.refresh();
  assert.equal(layout.shown('file_type'), 2);
  assert.equal(layout.canChoose(), true);
  assert.equal(layout.feedback.value, undefined);
  assert.equal(writes, 1);
});

test('a retired visit cannot publish a late write or readback in a new account or settings visit', async () => {
  for (const replacement of ['A', 'B']) {
    const held = Promise.withResolvers<DownloadLayoutSettings>();
    const { layout, calls, visit } = fixture({ change: () => held.promise });
    await layout.refresh();
    const saving = layout.choose('file_type', 2);
    visit(replacement);
    await layout.refresh();
    held.resolve({ fileType: 2, pathType: 1, revision: 'retired' });
    await saving;
    assert.equal(layout.shown('file_type'), 0);
    assert.equal(layout.activity.value, undefined);
    assert.deepEqual(calls, [
      ['read', 'A'],
      ['read', replacement],
    ]);
  }
});

test('invalid native layout is never treated as an editable default and can be reread', async () => {
  const invalid = { fileType: 1, pathType: 0, revision: null } as DownloadLayoutSettings;
  let reads = 0;
  const { layout, calls } = fixture({ read: async () => (++reads === 1 ? invalid : defaults) });
  await layout.refresh();
  assert.equal(layout.shown('file_type'), undefined);
  assert.equal(layout.feedback.value?.recheck, true);
  await layout.choose('file_type', 2);
  assert.deepEqual(calls, []);
  await layout.refresh();
  assert.equal(layout.shown('file_type'), 0);
  assert.equal(layout.canChoose(), true);
});

test('expired native account signals recovery without applying or rereading the expired layout', async () => {
  const { layout, calls } = fixture({
    change: async () => {
      throw { code: 'session_expired', message: 'expired' };
    },
  });
  await layout.refresh();
  await layout.choose('file_type', 2);
  assert.equal(layout.shown('file_type'), 0);
  assert.deepEqual(calls, [['read', 'A'], ['expired']]);
});
