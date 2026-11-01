import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, ref, shallowRef } from 'vue';
import { useMiniPin } from '../src/composables/useMiniPin.ts';
import type { MiniClient, MiniView, MiniPreferencesCommand } from '../src/services/mini.ts';

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function fixture(saved = true, applied: boolean | null = false) {
  const view = shallowRef<MiniView | null>({
    generation: '1',
    ticket: 'ticket',
    pinApplied: applied,
    preferences: { revision: 'saved-revision', alwaysOnTop: saved, damaged: false },
    media: {
      epoch: '1',
      selection: { audioPlayId: null, audioEntryId: null, videoPlayId: null },
      item: null,
      state: 'stopped',
      playing: false,
      canPrevious: false,
      canNext: false,
      position: null,
      duration: null,
      seekStart: 0,
      seekEnd: null,
      canSeek: false,
      volume: 100,
      mode: null,
      modeFamily: null,
      like: null,
      error: null,
    },
  });
  const busy = ref(false),
    calls: unknown[][] = [];
  let readFailure = false;
  let wait: Promise<void> | undefined;
  const update = (edit: Partial<MiniView>) => {
    view.value = { ...view.value!, ...edit };
  };
  const client: Pick<MiniClient, 'preferences' | 'applyPreferences'> = {
    async preferences(ticket: string, command: MiniPreferencesCommand) {
      calls.push(['save', ticket, command]);
      await wait;
    },
    async applyPreferences(ticket, revision) {
      calls.push(['apply', ticket, revision]);
      await wait;
    },
  };
  const scope = effectScope();
  const pin = scope.run(() =>
    useMiniPin(
      view,
      client,
      async () => {
        calls.push(['read']);
        if (readFailure) throw { code: 'storage', message: 'read failed', retryable: true };
        return view.value ?? undefined;
      },
      busy,
    ),
  )!;
  return {
    pin,
    view,
    busy,
    calls,
    client,
    scope,
    update,
    failRead: (failed: boolean) => {
      readFailure = failed;
    },
    hold: (promise: Promise<void>) => {
      wait = promise;
    },
  };
}

test('native pin drift retries the saved value without changing its revision or toggling', async () => {
  const f = fixture();
  assert.equal(f.pin.selected.value, false);
  assert.equal(f.pin.label.value, '重试应用置顶设置');
  await f.pin.apply();
  assert.deepEqual(f.calls, [['apply', 'ticket', 'saved-revision'], ['read']]);
  assert.equal(f.pin.selected.value, false); // No native observation of success yet.
  assert.equal(f.pin.label.value, '重试应用置顶设置');
  f.update({ pinApplied: true });
  assert.equal(f.pin.label.value, '始终置顶');
  assert.equal(f.pin.notice.value, '');
  f.scope.stop();
});

test('unpin intent and a busy retry cannot be reinterpreted as pin or a duplicate save', async () => {
  const f = fixture(false, true),
    held = deferred();
  f.hold(held.promise);
  const pending = f.pin.apply();
  await f.pin.apply();
  assert.equal(f.busy.value, true);
  assert.equal(f.pin.selected.value, true);
  assert.deepEqual(f.calls, [['apply', 'ticket', 'saved-revision']]);
  f.update({ pinApplied: false });
  held.resolve();
  await pending;
  assert.equal(f.pin.selected.value, false);
  assert.equal(f.pin.notice.value, '');
  assert.equal(f.calls.length, 2);
  f.scope.stop();
});

test('unknown native state and a failed refresh permit only explicit readonly recovery', async () => {
  const f = fixture(true, null);
  f.failRead(true);
  await f.pin.apply();
  assert.equal(f.pin.label.value, '重新读取置顶状态');
  f.update({ pinApplied: false }); // A later poll is not a confirmed explicit read.
  assert.equal(f.pin.label.value, '重新读取置顶状态');
  f.failRead(false);
  await f.pin.apply();
  assert.deepEqual(f.calls, [['read'], ['read']]);
  assert.equal(f.pin.label.value, '重试应用置顶设置');
  await f.pin.apply();
  assert.deepEqual(f.calls.slice(2), [['apply', 'ticket', 'saved-revision'], ['read']]);
  f.scope.stop();
});

test('save and following read failures remain distinct and cannot trigger another toggle', async () => {
  const f = fixture(true, true);
  f.client.preferences = async () => {
    f.calls.push(['rejected-save']);
    throw { code: 'storage', message: 'save failed' };
  };
  f.failRead(true);
  await f.pin.apply();
  assert.match(f.pin.notice.value, /save failed/);
  assert.match(f.pin.notice.value, /read failed/);
  f.failRead(false);
  await f.pin.apply();
  assert.deepEqual(f.calls, [['rejected-save'], ['read'], ['read']]);
  assert.equal(f.pin.label.value, '始终置顶');
  assert.equal(f.pin.notice.value, '');
  f.scope.stop();
});

test('healthy toggles and damaged repair are explicit writes; account changes discard late failure', async () => {
  const f = fixture(true, true);
  await f.pin.apply();
  assert.deepEqual(f.calls[0], [
    'save',
    'ticket',
    { kind: 'change', expected_revision: 'saved-revision', always_on_top: false },
  ]);
  f.update({ preferences: { revision: null, alwaysOnTop: false, damaged: true } });
  assert.equal(f.pin.label.value, '修复置顶设置');
  await f.pin.apply();
  assert.deepEqual(f.calls[2], ['save', 'ticket', { kind: 'repair' }]);
  const held = deferred();
  f.hold(held.promise);
  f.client.preferences = async () => {
    await held.promise;
    throw { code: 'storage', message: 'old account failed' };
  };
  const pending = f.pin.apply();
  f.update({
    pinApplied: true,
    preferences: { revision: 'other', alwaysOnTop: true, damaged: false },
    media: { ...f.view.value!.media, epoch: '2' },
  });
  await nextTick();
  held.resolve();
  await pending;
  assert.equal(f.pin.notice.value, '');
  assert.equal(f.busy.value, false);
  f.scope.stop();
});
