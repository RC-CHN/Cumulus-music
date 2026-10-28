import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSystemMediaPreferences } from '../src/composables/useSystemMediaPreferences.ts';
import { useSystemMediaStatus } from '../src/composables/useSystemMediaStatus.ts';
import type { AccountData, AccountView } from '../src/services/contracts.ts';
import type { SystemMediaPreferencesView, SystemMediaStatus } from '../src/services/systemMedia.ts';
const account = (epoch: string) => ({ epoch }) as AccountView;
const view = (revision = 'initial', enabled = true): SystemMediaPreferencesView => ({
  revision,
  enabled,
  damaged: false,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
test('lost preference acknowledgement reads actual state without resending or claiming success', async () => {
  let stored = view(),
    writes = 0,
    reads = 0;
  const model = useSystemMediaPreferences({
    systemMediaPreferences: async (epoch) => {
      reads++;
      return { epoch, value: stored };
    },
    systemMediaPreferencesCommand: async (_epoch, command) => {
      writes++;
      assert.equal(command.kind, 'change');
      stored = view('saved', false);
      throw { message: 'lost reply' };
    },
  });
  await model.connect(account('1'));
  assert.equal(await model.change(false), false);
  assert.equal(writes, 1);
  assert.equal(reads, 2);
  assert.equal(model.view.value?.enabled, false);
  assert.equal(model.saved.value, false);
  assert.equal(model.canChange.value, false);
  assert.ok(model.error.value);
  await model.read();
  assert.equal(model.canChange.value, true);
  assert.equal(writes, 1);
  model.dispose();
});
test('late old-account writes and reads cannot replace the current account setting view', async () => {
  const late = deferred<AccountData<SystemMediaPreferencesView>>();
  const model = useSystemMediaPreferences({
    systemMediaPreferences: async (epoch) => ({ epoch, value: view(epoch) }),
    systemMediaPreferencesCommand: () => late.promise,
  });
  await model.connect(account('1'));
  const saving = model.change(false);
  await model.connect(account('2'));
  late.resolve({ epoch: '1', value: view('old', false) });
  await saving;
  assert.equal(model.view.value?.revision, '2');
  assert.equal(model.view.value?.enabled, true);
  assert.equal(model.saved.value, false);
  model.dispose();
});
test('damaged state only permits explicit repair and preserves failure until refresh', async () => {
  let repaired = false,
    calls = 0;
  const model = useSystemMediaPreferences({
    systemMediaPreferences: async (epoch) => ({
      epoch,
      value: repaired ? view('fixed') : { revision: null, enabled: false, damaged: true },
    }),
    systemMediaPreferencesCommand: async (epoch, command) => {
      calls++;
      assert.equal(command.kind, 'repair');
      repaired = true;
      return { epoch, value: view('fixed') };
    },
  });
  await model.connect(account('1'));
  assert.equal(await model.change(true), false);
  assert.equal(calls, 0);
  assert.equal(await model.repair(), true);
  assert.equal(calls, 1);
  assert.equal(model.view.value?.damaged, false);
  assert.equal(model.saved.value, true);
  model.dispose();
});
test('new native status events outrank an older read and unsubscribe after disposal', async () => {
  const initial = deferred<SystemMediaStatus>();
  let handler!: (v: SystemMediaStatus) => void,
    removed = 0;
  const model = useSystemMediaStatus({
    systemMediaStatus: () => initial.promise,
    onSystemMediaStatus: async (callback) => {
      handler = callback;
      return () => {
        removed++;
      };
    },
  });
  const ready = model.connect();
  await Promise.resolve();
  handler({ revision: '9007199254740993', installed: true, phase: 'disabled' });
  initial.resolve({ revision: '9007199254740992', installed: true, phase: 'active' });
  await ready;
  assert.equal(model.view.value?.phase, 'disabled');
  model.dispose();
  assert.equal(removed, 1);
  handler({ revision: '9007199254740994', installed: true, phase: 'active' });
  assert.equal(model.view.value?.phase, 'disabled');
});
