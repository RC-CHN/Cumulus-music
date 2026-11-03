import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { effectScope } from 'vue';
import { useCloud } from '../src/composables/useCloud.ts';
import { useCloudTaskRemoval } from '../src/composables/useCloudTaskRemoval.ts';
import { cloudTaskTarget, taskProgress, uploadRowMenu } from '../src/services/cloudTasks.ts';
import type { CloudClient, CloudPage, CloudUpload } from '../src/services/cloud.ts';
const id = (n: number) => n.toString(16).padStart(32, '0');
function row(n = 1): CloudUpload {
  return {
    id: id(n),
    title: '合成任务',
    fileName: 'same.flac',
    phase: 'queued',
    bytes: 0,
    total: 100,
    label: '等待继续',
    enabled: false,
    running: false,
    error: null,
    canResume: true,
    canPause: false,
    canCancel: true,
    canReconcile: false,
    canRenew: false,
    canDismiss: false,
    canRemove: true,
    removalPending: false,
    attachments: [],
  };
}
function page(epoch = '1'): CloudPage {
  return {
    epoch,
    revision: 'r1',
    updatedMs: 1,
    size: 0,
    maxSize: 1000,
    uploadedCount: 0,
    uploadingCount: 0,
    historyCount: 0,
    total: 0,
    offset: 0,
    tracks: [],
    uploads: [],
    deletions: [],
    workerError: null,
  };
}
async function fixture(n = 2) {
  const scope = effectScope();
  let closes = 0,
    reads = 0;
  const commands: unknown[] = [];
  const records = new Map(
    Array.from({ length: n }, (_, i) => [
      id(i + 1),
      { id: id(i + 1), removed: false, requested: false, error: null as string | null },
    ]),
  );
  const api = {
    cloudPage: async (e: string) => page(e),
    cloudCommand: async (e: string, command: any) => {
      commands.push(structuredClone(command));
      for (const key of command.ids ?? []) {
        const item = records.get(key)!;
        item.requested = true;
        item.removed = true;
      }
      return { epoch: e, accepted: command.ids ?? [], failed: [], deletion: null };
    },
    cloudUploadRemovals: async (e: string, ids: string[]) => {
      reads++;
      return { epoch: e, items: ids.map((key) => ({ ...records.get(key)! })) };
    },
  };
  const model = useCloud(api as unknown as CloudClient);
  await model.connect({ epoch: '1', status: 'authenticated', nickname: null, avatarUrl: null });
  const target = cloudTaskTarget(
    '1',
    Array.from({ length: n }, (_, i) => row(i + 1)),
    true,
  );
  const state = scope.run(() => useCloudTaskRemoval(model, api, target, () => closes++))!;
  return { scope, model, api, state, target, commands, records, counts: () => ({ closes, reads }) };
}
test('task row menu order and disabled playback match executed recovered 622.We', () => {
  const oracle = JSON.parse(
    readFileSync(
      new URL('../../core/tests/fixtures/cloud-upload-ui.json', import.meta.url),
      'utf8',
    ),
  );
  assert.deepEqual(
    uploadRowMenu(false).map((row) =>
      row.separator ? { separator: true } : { label: row.label, disabled: !!row.disabled },
    ),
    oracle.rows.map((row: any) =>
      row.separator ? { separator: true } : { label: row.label, disabled: row.disabled },
    ),
  );
  assert(
    uploadRowMenu(true)
      .filter((row) => !row.separator)
      .every((row) => row.disabled),
  );
});
test('progress combines acknowledged audio and attachments and separates processing, pause and unknown removal', () => {
  const task = row();
  task.phase = 'attachments_uploading';
  task.bytes = 100;
  task.enabled = true;
  task.attachments = [{ kind: 'cover', phase: 'uploading', bytes: 10, total: 100 }];
  assert(Math.abs(taskProgress(task).percent - 55) < 0.000001);
  assert.equal(taskProgress(task).label, '55.0%');
  task.enabled = false;
  assert.equal(taskProgress(task).label, '已暂停');
  task.phase = 'encoding';
  task.enabled = true;
  assert.equal(taskProgress(task).label, '正在转码');
  task.phase = 'failed';
  task.error = 'encoding_failed';
  assert.equal(taskProgress(task).label, '转码失败');
  task.removalPending = true;
  task.label = '等待核对';
  assert.equal(taskProgress(task).label, '等待核对');
});
test('cloud song filters, order and page survive task/history navigation without filtering the task scope', async () => {
  const model = useCloud({
    cloudPage: async (e: string, q: any) => ({ ...page(e), total: 405, offset: q.offset }),
  } as CloudClient);
  await model.connect({ epoch: '1', status: 'authenticated', nickname: null, avatarUrl: null });
  model.filter({ keyword: '保留', sort: 'title', descending: false, offset: 200 });
  model.filter({ tab: 'uploading' });
  assert.equal(model.query.value.keyword, '');
  assert.equal(model.query.value.offset, 0);
  assert.equal(model.query.value.sort, 'natural');
  model.filter({ offset: 200 });
  model.filter({ tab: 'history' });
  assert.equal(model.query.value.offset, 0);
  model.filter({ tab: 'uploading' });
  assert.equal(model.query.value.offset, 200);
  model.filter({ tab: 'uploaded' });
  assert.equal(model.query.value.keyword, '保留');
  assert.equal(model.query.value.sort, 'title');
  assert.equal(model.query.value.descending, false);
  assert.equal(model.query.value.offset, 200);
  model.dispose();
});
test('confirmation captures only its original task IDs and removal is confirmed through native receipts', async () => {
  const f = await fixture();
  f.records.set(id(3), { id: id(3), removed: false, requested: false, error: null });
  await f.state.send();
  assert.deepEqual(f.commands, [{ kind: 'remove_uploads', ids: [id(1), id(2)], retry: false }]);
  assert.equal(f.counts().closes, 1);
  assert.equal(f.state.removed.value, 2);
  assert.equal(f.records.get(id(3))!.removed, false);
  f.scope.stop();
  f.model.dispose();
});
test('a lost write reply reads receipts, never resends automatically, and explicit retry retains the same scope', async () => {
  const f = await fixture();
  const actual = f.api.cloudCommand;
  f.api.cloudCommand = async (e, c) => {
    await actual(e, c);
    throw { code: 'io', message: 'lost reply' };
  };
  await f.state.send();
  assert.equal(f.commands.length, 1);
  assert.equal(f.counts().closes, 1);
  assert.equal(f.state.removed.value, 2);
  assert.equal(f.model.error.value, '');
  f.scope.stop();
  f.model.dispose();
});
test('partial cancellation remains visible and read-only checks never issue another mutation', async () => {
  const f = await fixture();
  f.api.cloudCommand = async (e, command) => {
    f.commands.push(structuredClone(command));
    f.records.get(id(1))!.removed = true;
    return {
      epoch: e,
      accepted: [id(1)],
      failed: [{ id: id(2), message: '等待核对' }],
      deletion: null,
    } as any;
  };
  await f.state.send();
  assert.equal(f.state.removed.value, 1);
  assert.equal(f.counts().closes, 0);
  assert.equal(f.state.failures.value[id(2)], '等待核对');
  await f.state.read();
  assert.equal(f.commands.length, 1);
  f.api.cloudCommand = async (e, command) => {
    f.commands.push(structuredClone(command));
    f.records.get(id(2))!.removed = true;
    return { epoch: e, accepted: [id(2)], failed: [], deletion: null };
  };
  await f.state.send();
  assert.deepEqual(f.commands[1], { kind: 'remove_uploads', ids: [id(2)], retry: true });
  assert.equal(f.counts().closes, 1);
  f.scope.stop();
  f.model.dispose();
});
test('account replacement discards late receipts and never sends the next 5000-task chunk', async () => {
  const f = await fixture(5001);
  let release!: () => void;
  f.api.cloudCommand = async (e, c) => {
    f.commands.push(c);
    await new Promise<void>((r) => (release = r));
    return { epoch: e, accepted: c.ids, failed: [], deletion: null };
  };
  const pending = f.state.send();
  await f.model.connect({ epoch: '2', status: 'authenticated', nickname: null, avatarUrl: null });
  release();
  await pending;
  assert.equal(f.commands.length, 1);
  assert.equal(f.counts().reads, 0);
  assert.equal(f.counts().closes, 0);
  f.scope.stop();
  f.model.dispose();
});

test('command receipts and errors do not reopen task feedback after changing the displayed tab', async () => {
  for (const rejects of [false, true]) {
    let release!: (v: unknown) => void;
    const model = useCloud({
      cloudPage: async (e: string) => page(e),
      cloudCommand: async () => {
        await new Promise((r) => (release = r));
        if (rejects) throw { code: 'io', message: 'old task failure' };
        return { epoch: '1', accepted: [id(1)], failed: [], deletion: null };
      },
    } as CloudClient);
    await model.connect({ epoch: '1', status: 'authenticated', nickname: null, avatarUrl: null });
    model.filter({ tab: 'uploading' });
    const command = model.command({ kind: 'pause_all' });
    model.filter({ tab: 'uploaded' });
    release(undefined);
    await command;
    assert.equal(model.notice.value, '');
    assert.equal(model.error.value, '');
    model.dispose();
  }
});

test('omitted text does not reduce upload completion for retained attachments', () => {
  const task = row();
  task.bytes = task.total;
  task.attachments = [
    { kind: 'cover', phase: 'uploaded', bytes: 30, total: 30 },
    { kind: 'lyric', phase: 'omitted', bytes: 0, total: 50 },
    { kind: 'cue', phase: 'queued', bytes: 0, total: 20 },
  ];
  assert.equal(taskProgress(task).total, 150);
  assert.equal(taskProgress(task).bytes, 130);
  task.attachments[2] = { kind: 'cue', phase: 'uploaded', bytes: 20, total: 20 };
  assert.equal(taskProgress(task).percent, 100);
});
