import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, ref } from 'vue';
import { useLyricReport } from '../src/composables/useLyricReport.ts';
import type { LyricReport, LyricReportClient } from '../src/services/lyricReports.ts';
function deferred<T>() {
  let resolve!: (v: T) => void, reject!: (v: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const epoch = ref('1'),
    calls: unknown[][] = [],
    messages: string[] = [];
  let value: LyricReport = {
    epoch: '1',
    id: 'report1',
    revision: 'r1',
    title: '原歌曲',
    options: [{ key: '4', label: '歌词有误', value: 4 }],
    selected: null,
    content: '',
    phase: 'draft',
    issue: null,
  };
  let opening: Promise<LyricReport> | undefined,
    reading: Promise<LyricReport> | undefined,
    typing: Promise<LyricReport> | undefined,
    writing: Promise<LyricReport> | undefined;
  const client: LyricReportClient = {
    open: async (...args) => {
      calls.push(['open', ...args]);
      return opening ?? value;
    },
    read: async (...args) => {
      calls.push(['read', ...args]);
      return reading ?? value;
    },
    types: async (...args) => {
      calls.push(['types', ...args]);
      return typing ?? value;
    },
    submit: async (...args) => {
      calls.push(['submit', ...args]);
      return writing ?? value;
    },
    abandon: async (...args) => {
      calls.push(['abandon', ...args]);
      return { ...value, phase: 'abandoned' };
    },
  };
  const scope = effectScope(),
    model = scope.run(() =>
      useLyricReport(
        () => epoch.value,
        client,
        (v) => messages.push(v),
      ),
    )!;
  return {
    epoch,
    calls,
    messages,
    scope,
    model,
    get value() {
      return value;
    },
    set value(v: LyricReport) {
      value = v;
    },
    opening: (v: Promise<LyricReport>) => {
      opening = v;
    },
    reading: (v: Promise<LyricReport>) => {
      reading = v;
    },
    typing: (v: Promise<LyricReport>) => {
      typing = v;
    },
    writing: (v: Promise<LyricReport>) => {
      writing = v;
    },
  };
}
function form(f: ReturnType<typeof fixture>) {
  f.model.selected.value = '4';
  f.model.content.value = '歌词内容有误';
}
test('delayed opening and type reads cannot reopen a closed or replaced account form', async () => {
  const f = fixture(),
    held = deferred<LyricReport>();
  f.opening(held.promise);
  const first = f.model.show('play1');
  f.model.close();
  held.resolve(f.value);
  await first;
  assert.equal(f.model.open.value, false);
  assert.equal(f.model.record.value, undefined);
  const type = deferred<LyricReport>();
  f.typing(type.promise);
  const next = f.model.show('play2');
  await nextTick();
  f.epoch.value = '2';
  await nextTick();
  type.resolve(f.value);
  await next;
  assert.equal(f.model.open.value, false);
  assert.equal(f.model.record.value, undefined);
  assert.equal(f.model.ownerEpoch.value, '1');
  f.scope.stop();
});
test('lost acknowledgement retains the form, blocks resend and performs only one read', async () => {
  const f = fixture();
  await f.model.show('play1');
  form(f);
  f.writing(Promise.reject({ code: 'transport', message: 'reply lost' }));
  f.reading(Promise.reject({ code: 'storage', message: 'read lost' }));
  await f.model.submit();
  assert.equal(f.model.content.value, '歌词内容有误');
  assert.equal(f.model.selected.value, '4');
  assert.equal(f.model.uncertain.value, true);
  assert.equal(f.model.canSubmit.value, false);
  await f.model.submit();
  assert.equal(f.calls.filter((v) => v[0] === 'submit').length, 1);
  assert.equal(f.calls.filter((v) => v[0] === 'read').length, 2);
  assert.deepEqual(f.messages, []);
  f.scope.stop();
});
test('a rejected or not dispatched write keeps text when readback is still a draft', async () => {
  const f = fixture();
  await f.model.show('play1');
  form(f);
  f.writing(Promise.reject({ code: 'stale', message: 'context changed' }));
  await f.model.submit();
  assert.equal(f.model.content.value, '歌词内容有误');
  assert.equal(f.model.selected.value, '4');
  assert.equal(f.model.canSubmit.value, true);
  assert.equal(f.calls.filter((v) => v[0] === 'submit').length, 1);
  f.scope.stop();
});
test('known rejection preserves choice and a later explicit send uses the new revision', async () => {
  const f = fixture();
  await f.model.show('play1');
  form(f);
  f.writing(
    Promise.resolve({
      ...f.value,
      phase: 'rejected',
      issue: 'rejected',
      revision: 'r2',
      selected: '4',
      content: '歌词内容有误',
    }),
  );
  await f.model.submit();
  assert.equal(f.model.open.value, true);
  assert.equal(f.model.canSubmit.value, true);
  assert.equal(f.model.content.value, '歌词内容有误');
  assert.deepEqual(f.messages, []);
  f.writing(Promise.resolve({ ...f.value, phase: 'sent', revision: 'r3' }));
  await f.model.submit();
  assert.equal(f.model.open.value, false);
  assert.equal(f.messages.length, 1);
  const calls = f.calls.filter((v) => v[0] === 'submit');
  assert.equal(calls.length, 2);
  assert.equal(calls[1][3], 'r2');
  f.scope.stop();
});
test('reloading changed types preserves description, clears a removed choice and respects UTF-16', async () => {
  const f = fixture();
  await f.model.show('play1');
  form(f);
  f.typing(
    Promise.resolve({
      ...f.value,
      revision: 'r2',
      options: [{ key: '5', label: '歌词不同步', value: 5 }],
    }),
  );
  await f.model.types();
  assert.equal(f.model.content.value, '歌词内容有误');
  assert.equal(f.model.selected.value, '');
  assert.equal(f.model.canSubmit.value, false);
  f.model.selected.value = '5';
  f.model.content.value = '😀'.repeat(25);
  assert.equal(f.model.canSubmit.value, true);
  f.model.content.value = '😀'.repeat(26);
  assert.equal(f.model.canSubmit.value, false);
  f.scope.stop();
});
test('late write results do not close or notify a newer account form', async () => {
  const f = fixture(),
    held = deferred<LyricReport>();
  await f.model.show('play1');
  form(f);
  f.writing(held.promise);
  const send = f.model.submit();
  f.epoch.value = '2';
  await nextTick();
  f.value = { ...f.value, epoch: '2', id: 'report2', title: '新账号歌曲' };
  await f.model.show('play2');
  held.resolve({ ...f.value, epoch: '1', id: 'report1', phase: 'sent' });
  await send;
  assert.equal(f.model.open.value, true);
  assert.equal(f.model.record.value?.id, 'report2');
  assert.equal(f.model.working.value, false);
  assert.deepEqual(f.messages, []);
  f.scope.stop();
});
test('type retry first reads the latest native revision and preserves the user description', async () => {
  const f = fixture();
  await f.model.show('play1');
  form(f);
  f.value = { ...f.value, revision: 'external-r2' };
  f.typing(Promise.resolve({ ...f.value, revision: 'types-r3' }));
  await f.model.types();
  assert.equal(f.calls.filter((v) => v[0] === 'types').at(-1)?.[3], 'external-r2');
  assert.equal(f.model.record.value?.revision, 'types-r3');
  assert.equal(f.model.content.value, '歌词内容有误');
  assert.equal(f.calls.filter((v) => v[0] === 'submit').length, 0);
  f.scope.stop();
});
