import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useShortcutSettings } from '../src/composables/useShortcutSettings.ts';
import { useShortcutStatus } from '../src/composables/useShortcutStatus.ts';
import { useLocalShortcuts } from '../src/composables/useLocalShortcuts.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type {
  KeyCapture,
  KeyInput,
  ShortcutPreferencesClient,
  ShortcutSettingsView,
  ShortcutStatus,
  ShortcutLease,
} from '../src/services/shortcuts.ts';
const account = (epoch: string) => ({ epoch }) as AccountView;
const input = (keyCode: number): KeyInput => ({
  keyCode,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
  metaKey: false,
});
const view = (revision = 'initial'): ShortcutSettingsView => ({
  current: {
    revision,
    global_enabled: true,
    media_enabled: true,
    bindings: { play_pause: { local: [17, 80], global: [17, 18, 80] } },
  },
  damaged: false,
});
const status = (revision: string, ticket = 'ticket'): ShortcutStatus => ({
  revision,
  ticket,
  recording: null,
  settingsRevision: 'initial',
  phase: 'ready',
  rows: [],
});
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const tick = async () => {
  for (let i = 0; i < 8; i++) await Promise.resolve();
};
function fixture(overrides: Partial<ShortcutPreferencesClient> = {}) {
  const ended: string[] = [];
  const core: ShortcutPreferencesClient = {
    shortcutSettings: async (epoch) => ({ epoch, value: view() }),
    shortcutSettingsCommand: async (epoch) => ({ epoch, value: view('saved') }),
    shortcutCapture: async (key) => ({ kind: 'set', keys: [key.keyCode] }),
    shortcutBeginRecording: async () => 'recording',
    shortcutEndRecording: async (id) => {
      ended.push(id);
    },
    ...overrides,
  };
  return { model: useShortcutSettings(core), core, ended };
}
test('late recording acquisition is released after account change, cancellation or disposal', async () => {
  for (const action of ['account', 'cancel', 'dispose']) {
    const pending = deferred<string>();
    const { model, ended } = fixture({ shortcutBeginRecording: () => pending.promise });
    await model.connect(account('1'));
    const opening = model.beginRecording('play_pause', 'local');
    if (action === 'account') await model.connect(account('2'));
    else if (action === 'cancel') await model.cancelRecording();
    else model.dispose();
    pending.resolve('late');
    await opening;
    assert.deepEqual(ended, ['late']);
    assert.equal(model.draft.value, undefined);
    assert.equal(model.error.value, '');
    model.dispose();
  }
});
test('older capture success or failure cannot overwrite the latest chord', async () => {
  for (const fails of [false, true]) {
    const pending = deferred<KeyCapture>();
    const { model } = fixture({
      shortcutCapture: (key) =>
        key.keyCode === 79
          ? pending.promise
          : Promise.resolve({ kind: 'set', keys: [key.keyCode] }),
    });
    await model.connect(account('1'));
    await model.beginRecording('play_pause', 'local');
    const older = model.capture(input(79));
    await model.capture(input(81));
    if (fails) pending.reject({ message: 'late failure' });
    else pending.resolve({ kind: 'set', keys: [79] });
    await older;
    assert.deepEqual(model.draft.value?.keys, [81]);
    assert.equal(model.recordError.value, '');
    model.dispose();
  }
});
test('lost write acknowledgement reads saved state and releases recording without replay or success claim', async () => {
  let saved = view(),
    writes = 0,
    reads = 0;
  const { model, ended } = fixture({
    shortcutSettings: async (epoch) => {
      reads++;
      return { epoch, value: saved };
    },
    shortcutSettingsCommand: async (_epoch, command) => {
      writes++;
      assert.equal(command.kind, 'change');
      if (command.kind === 'change') assert.equal(command.expected_revision, 'initial');
      saved = view('committed');
      throw { message: 'lost reply' };
    },
  });
  await model.connect(account('1'));
  await model.beginRecording('play_pause', 'local');
  await model.capture(input(79));
  assert.equal(await model.saveRecording(), false);
  assert.equal(writes, 1);
  assert.equal(reads, 2);
  assert.equal(model.view.value?.current?.revision, 'committed');
  assert.equal(model.saved.value, false);
  assert.deepEqual(ended, ['recording']);
  assert.equal(model.canChange.value, false);
  await model.read();
  assert.equal(model.canChange.value, true);
  assert.equal(writes, 1);
  model.dispose();
});
test('conflict keeps original revision and draft; an external revision requires a new explicit edit', async () => {
  let stored = view(),
    sent: string[] = [];
  const { model, ended } = fixture({
    shortcutSettings: async (epoch) => ({ epoch, value: stored }),
    shortcutSettingsCommand: async (_epoch, command) => {
      assert.equal(command.kind, 'change');
      if (command.kind === 'change') sent.push(command.expected_revision);
      throw { message: 'conflict' };
    },
  });
  await model.connect(account('1'));
  await model.beginRecording('play_pause', 'local');
  await model.capture(input(79));
  assert.equal(await model.saveRecording(), false);
  assert.equal(model.draft.value?.revision, 'initial');
  assert.deepEqual(ended, []);
  stored = view('other-writer');
  await model.capture(input(81));
  await model.saveRecording();
  assert.deepEqual(sent, ['initial', 'initial']);
  assert.equal(model.draft.value, undefined);
  assert.deepEqual(ended, ['recording']);
  model.dispose();
});
test('late recording cleanup failure does not contaminate another account', async () => {
  const pending = deferred<void>();
  const { model } = fixture({ shortcutEndRecording: () => pending.promise });
  await model.connect(account('1'));
  const ending = model.resumeRecording('orphan');
  await model.connect(account('2'));
  pending.reject({ message: 'old failure' });
  await ending;
  assert.equal(model.error.value, '');
  model.dispose();
});
test('recording expiration closes the draft without persisting a captured chord', async () => {
  let writes = 0;
  const { model } = fixture({
    shortcutSettingsCommand: async (epoch) => {
      writes++;
      return { epoch, value: view() };
    },
  });
  await model.connect(account('1'));
  await model.beginRecording('play_pause', 'local');
  await model.capture(input(79));
  model.observeNative({ ...status('2'), phase: 'starting' });
  assert.ok(model.draft.value);
  model.observeNative(status('3'));
  await tick();
  assert.equal(model.draft.value, undefined);
  assert.equal(writes, 0);
  assert.ok(model.error.value);
  model.dispose();
});
test('new native events outrank stale reads using full integer precision', async () => {
  const initial = deferred<ShortcutStatus>();
  let callback!: (value: ShortcutStatus) => void,
    removed = 0;
  const model = useShortcutStatus({
    shortcutStatus: () => initial.promise,
    onShortcuts: async (handler) => {
      callback = handler;
      return () => {
        removed++;
      };
    },
  });
  const ready = model.connect();
  await tick();
  callback({ ...status('9007199254740993'), phase: 'recording' });
  initial.resolve(status('9007199254740992'));
  await ready;
  assert.equal(model.view.value?.phase, 'recording');
  model.dispose();
  assert.equal(removed, 1);
  callback(status('9007199254740994'));
  assert.equal(model.view.value?.phase, 'recording');
});
test('late listener acquisitions release only their own lease across account changes', async () => {
  const old = deferred<ShortcutLease>(),
    detached: string[] = [];
  let acquisitions = 0;
  const model = useLocalShortcuts(() => assert.fail('unexpected shortcut error'), {
    shortcutStatus: async () => status('1'),
    onShortcuts: async () => () => {},
    onShortcutError: async () => () => {},
    shortcutAttach: async (epoch) => {
      acquisitions++;
      return epoch === '1' ? old.promise : { id: 'new', ticket: 'ticket' };
    },
    shortcutDetach: async (id) => {
      detached.push(id);
    },
    shortcutPress: async () => {},
  });
  await model.connect('1');
  model.account('2');
  await tick();
  old.resolve({ id: 'old', ticket: 'ticket' });
  await tick();
  assert.equal(acquisitions, 2);
  assert.deepEqual(detached, ['old']);
  model.dispose();
  assert.deepEqual(detached, ['old', 'new']);
});
