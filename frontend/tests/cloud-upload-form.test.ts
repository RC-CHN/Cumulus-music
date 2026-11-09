import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, ref } from 'vue';
import { useCloudUploadForm } from '../src/composables/useCloudUploadForm.ts';
import {
  uploadBasename,
  uploadRequestId,
  validUploadReceipt,
} from '../src/services/cloudUploadForm.ts';
import type { CloudCommand, CloudResult } from '../src/services/cloud.ts';
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function fixture() {
  const scope = effectScope(),
    accountEpoch = ref('1'),
    busy = ref(false),
    error = ref('');
  let ids = 0,
    started = 0,
    closed = 0;
  const calls: CloudCommand[] = [];
  const backend = {
    pick: async (_kind: string): Promise<string[]> => [],
    command: async (_command: CloudCommand): Promise<CloudResult | undefined> => undefined,
  };
  const model = {
    accountEpoch,
    busy,
    error,
    pick: (kind: string) => backend.pick(kind),
    command: async (command: CloudCommand) => {
      calls.push(structuredClone(command));
      return backend.command(command);
    },
  };
  const state = scope.run(() =>
    useCloudUploadForm(model, { started: () => started++, close: () => closed++ }, () =>
      (++ids).toString(16).padStart(32, '0'),
    ),
  )!;
  return { scope, model, state, calls, backend, counts: () => ({ started, closed }) };
}
function reply(accepted: string[], failed: { id: string; message: string }[] = []): CloudResult {
  return { epoch: '1', accepted, failed, deletion: null };
}
const task = 'a'.repeat(32);
test('partial registration shows exact failed paths and retries only those with stable request IDs', async () => {
  const f = fixture();
  f.backend.pick = async () => ['/first/same.flac', '/second/same.flac', '/first/same.flac'];
  await f.state.pick('audio');
  assert.equal(f.state.rows.value.length, 2);
  const original = structuredClone(
    f.state.rows.value.map((r) => ({ requestId: r.requestId, path: r.input.path })),
  );
  f.backend.command = async () =>
    reply([task], [{ id: '/second/same.flac', message: '合成附件无效' }]);
  await f.state.submit();
  assert.equal(f.state.rows.value.length, 1);
  assert.equal(f.state.rows.value[0]!.input.path, '/second/same.flac');
  assert.equal(f.state.rows.value[0]!.error, '合成附件无效');
  assert.equal(f.state.rows.value[0]!.requestId, original[1]!.requestId);
  assert.deepEqual(f.counts(), { started: 1, closed: 0 });
  f.backend.command = async () => reply(['b'.repeat(32)]);
  await f.state.submit();
  assert.equal(f.calls[1]!.kind, 'upload_planned');
  if (f.calls[1]!.kind === 'upload_planned')
    assert.deepEqual(
      f.calls[1]!.files.map((v) => v.input.path),
      ['/second/same.flac'],
    );
  assert.deepEqual(f.counts(), { started: 2, closed: 1 });
  f.scope.stop();
});
test('missing or malformed receipts freeze selection and explicit recovery uses the same IDs and attachments', async () => {
  for (const mode of ['missing', 'throw', 'wrong_epoch', 'duplicate_failure']) {
    const f = fixture();
    f.backend.pick = async (kind) => (kind === 'audio' ? ['/audio.flac'] : ['/cover.png']);
    await f.state.pick('audio');
    await f.state.attachment(f.state.rows.value[0]!, 'cover');
    f.backend.command = async () => {
      if (mode === 'throw') throw new Error('synthetic lost reply');
      if (mode === 'wrong_epoch') return { ...reply([task]), epoch: '2' };
      if (mode === 'duplicate_failure')
        return reply(
          [],
          [
            { id: '/audio.flac', message: 'x' },
            { id: '/audio.flac', message: 'x' },
          ],
        );
      return undefined;
    };
    await f.state.submit();
    assert(f.state.uncertain.value);
    assert(!f.state.editable.value);
    const original = f.calls[0]!;
    f.state.remove(f.state.rows.value[0]!);
    f.state.clearAttachment(f.state.rows.value[0]!, 'cover');
    await f.state.pick('folder');
    assert.equal(f.state.rows.value[0]!.input.cover, '/cover.png');
    assert.equal(f.state.rows.value.length, 1);
    f.backend.command = async () => reply([task]);
    await f.state.submit();
    assert.deepEqual(f.calls[1], original);
    assert(!f.state.uncertain.value);
    assert.deepEqual(f.counts(), { started: 1, closed: 1 });
    f.scope.stop();
  }
});
test('attachment cancellation preserves receipt, a changed attachment resets it, and duplicate picks keep rows', async () => {
  const f = fixture();
  let selected = ['/music.flac'];
  f.backend.pick = async () => selected;
  await f.state.pick('audio');
  const row = f.state.rows.value[0]!,
    initial = row.requestId;
  await f.state.pick('audio');
  assert.equal(f.state.rows.value[0], row);
  selected = [];
  await f.state.attachment(row, 'cover');
  assert.equal(row.requestId, initial);
  selected = ['/cover.png'];
  await f.state.attachment(row, 'cover');
  const attached = row.requestId;
  assert.notEqual(attached, initial);
  assert.equal(row.input.cover, '/cover.png');
  await f.state.attachment(row, 'cover');
  assert.equal(row.requestId, attached);
  row.error = 'old';
  f.state.clearAttachment(row, 'cover');
  assert.notEqual(row.requestId, attached);
  assert.equal(row.input.cover, null);
  assert.equal(row.error, '');
  f.scope.stop();
});
test('aggregate selection is bounded at 500, displays 20, and refuses an oversized addition atomically', async () => {
  const f = fixture();
  f.backend.pick = async () => Array.from({ length: 500 }, (_, i) => `/music/${i}.flac`);
  await f.state.pick('audio');
  assert.equal(f.state.rows.value.length, 500);
  assert.equal(f.state.visible.value.length, 20);
  f.state.page.value = 24;
  assert.equal(f.state.visible.value[0]!.input.path, '/music/480.flac');
  f.backend.pick = async () => ['/extra.flac', '/music/0.flac'];
  await f.state.pick('folder');
  assert.equal(f.state.rows.value.length, 500);
  assert.match(f.state.error.value, /500/);
  f.state.remove(f.state.rows.value[499]!);
  assert.equal(f.state.error.value, '');
  await nextTick();
  assert.equal(f.state.visible.value.length, 19);
  f.backend.command = async (c) =>
    c.kind === 'upload_planned'
      ? reply(c.files.map((_, i) => (i + 1).toString(16).padStart(32, '0')))
      : undefined;
  await f.state.submit();
  assert.equal(f.state.rows.value.length, 0);
  assert.equal(f.state.page.value, 0);
  f.scope.stop();
});
test('one pending picker or registration disables overlapping operations but preserves explicit cancel', async () => {
  const f = fixture(),
    choice = deferred<string[]>();
  let picks = 0;
  f.backend.pick = async () => {
    picks++;
    return choice.promise;
  };
  const pending = f.state.pick('audio');
  await f.state.pick('folder');
  await f.state.submit();
  assert.equal(picks, 1);
  assert(f.state.blocked.value);
  assert.equal(f.calls.length, 0);
  choice.resolve(['/music.flac']);
  await pending;
  const saved = deferred<CloudResult>();
  f.backend.command = async () => saved.promise;
  const submitting = f.state.submit();
  await f.state.submit();
  await f.state.pick('audio');
  assert.equal(f.calls.length, 1);
  assert.equal(picks, 1);
  saved.resolve(reply([task]));
  await submitting;
  assert(!f.state.blocked.value);
  f.scope.stop();
});
test('account changes and disposal discard late picker/registration replies and callbacks', async () => {
  for (const kind of ['pick', 'submit'])
    for (const replace of ['account', 'dispose']) {
      const f = fixture(),
        held = deferred<string[] | CloudResult>();
      let pending: Promise<void>;
      if (kind === 'pick') {
        f.backend.pick = async () => held.promise as Promise<string[]>;
        pending = f.state.pick('audio');
      } else {
        f.backend.pick = async () => ['/private/music.flac'];
        await f.state.pick('audio');
        f.backend.command = async () => held.promise as Promise<CloudResult>;
        pending = f.state.submit();
      }
      if (replace === 'account') f.model.accountEpoch.value = '2';
      else f.scope.stop();
      assert.equal(f.state.rows.value.length, 0);
      held.resolve(kind === 'pick' ? ['/private/late.flac'] : reply([task]));
      await pending;
      assert.equal(f.state.rows.value.length, 0);
      assert.deepEqual(f.counts(), { started: 0, closed: 0 });
      f.scope.stop();
    }
});
test('a registered task that did not start is not resubmitted as a failed registration', async () => {
  const f = fixture();
  f.backend.pick = async () => ['/music.flac'];
  await f.state.pick('audio');
  f.backend.command = async () => ({
    ...reply([task]),
    notStarted: [{ id: '/music.flac', message: 'paused' }],
  });
  await f.state.submit();
  assert.equal(f.state.rows.value.length, 0);
  assert.match(f.state.message.value, /尚未开始/);
  assert.deepEqual(f.counts(), { started: 1, closed: 1 });
  f.scope.stop();
});
test('receipt coverage rejects unknown/duplicate not-started paths and cross-platform basenames stay distinct by full path', () => {
  const rows = [
    {
      requestId: 'b'.repeat(32),
      input: { path: '/a.flac', cover: null, lyric: null, cue: null },
      error: '',
    },
  ];
  assert(validUploadReceipt(reply([task]), rows, '1'));
  for (const notStarted of [
    [{ id: '/other.flac', message: 'x' }],
    [
      { id: '/a.flac', message: 'x' },
      { id: '/a.flac', message: 'x' },
    ],
  ])
    assert(!validUploadReceipt({ ...reply([task]), notStarted }, rows, '1'));
  assert.equal(uploadBasename('C:\\Music\\same.flac'), 'same.flac');
  assert.equal(uploadBasename('/Music/same.flac'), 'same.flac');
  assert.match(uploadRequestId(), /^[a-f0-9]{32}$/);
});

test('native size filtering distinguishes partial and all-rejected input without resubmitting an admitted file', async () => {
  const f = fixture();
  f.backend.pick = async () => ['/large.flac', '/valid.flac'];
  await f.state.pick('folder');
  const rejected = [{ id: '/large.flac', message: '文件超过100M，无法上传' }];
  f.backend.command = async () => ({ ...reply([task], rejected), uploadLimit: 'partial' });
  await f.state.submit();
  assert.match(f.state.message.value, /^部分文件超过100M，已经过滤/);
  assert.equal(f.state.error.value, '');
  assert.deepEqual(f.counts(), { started: 1, closed: 0 });
  assert.equal(f.state.rows.value[0]?.error, rejected[0]?.message);
  f.backend.command = async () => ({ ...reply([], rejected), uploadLimit: 'no_new_tasks' });
  await f.state.submit();
  assert.equal(f.state.error.value, '文件超过100M，无法上传');
  assert.equal(f.state.message.value, '');
  assert.deepEqual(f.counts(), { started: 1, closed: 0 });
  const retry = f.calls[1];
  assert(retry?.kind === 'upload_planned');
  assert.deepEqual(
    retry.files.map((v) => v.input.path),
    ['/large.flac'],
  );
  f.scope.stop();
});
