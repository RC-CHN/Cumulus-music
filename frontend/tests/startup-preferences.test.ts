import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useStartupPreferences } from '../src/composables/useStartupPreferences.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type { StartupPreferencesClient, StartupPreferencesView } from '../src/services/startup.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'signed_out' as const,
  nickname: '',
  avatarUrl: null,
});
const value = (): StartupPreferencesView => ({
  revision: '0',
  settings: { autoPlay: false, rememberPosition: true },
  damaged: false,
});
const reply = (view = value(), epoch = '1') => ({ value: structuredClone(view), epoch });
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function backend(extra: Partial<StartupPreferencesClient> = {}): StartupPreferencesClient {
  return {
    startupPreferences: async (e) => reply(value(), e),
    startupPreferencesCommand: async (e) => reply(value(), e),
    ...extra,
  };
}
test('startup settings wait for native persistence and do not optimistically flip or repeat clicks', async () => {
  const gate = deferred<AccountData<StartupPreferencesView>>();
  let calls = 0;
  const model = useStartupPreferences(
    backend({
      startupPreferencesCommand: async (_, command) => {
        calls++;
        assert.equal(command.kind, 'change');
        return gate.promise;
      },
    }),
  );
  await model.connect(account());
  const work = model.change('autoPlay', true);
  assert.equal(model.view.value?.settings.autoPlay, false);
  assert.equal(await model.change('rememberPosition', false), false);
  gate.resolve(
    reply({ ...value(), revision: '1', settings: { autoPlay: true, rememberPosition: true } }),
  );
  assert.equal(await work, true);
  assert.equal(model.saved.value, true);
  assert.equal(calls, 1);
});
test('lost local write acknowledgement reads current state without replay or claiming success', async () => {
  let stored = value(),
    calls = 0;
  const model = useStartupPreferences(
    backend({
      startupPreferences: async () => reply(stored),
      startupPreferencesCommand: async () => {
        calls++;
        stored = {
          ...value(),
          revision: '1',
          settings: { autoPlay: true, rememberPosition: true },
        };
        throw { message: 'lost reply', code: 'network' };
      },
    }),
  );
  await model.connect(account());
  assert.equal(await model.change('autoPlay', true), false);
  assert.equal(model.view.value?.settings.autoPlay, true);
  assert.equal(model.saved.value, false);
  assert.ok(model.error.value);
  await model.change('autoPlay', false);
  assert.equal(calls, 1);
  await model.read();
  assert.equal(model.canChange.value, true);
});
test('account change and disposal reject late preference reads and command responses', async () => {
  const old = deferred<AccountData<StartupPreferencesView>>();
  const model = useStartupPreferences(
    backend({
      startupPreferences: (e) => (e === '1' ? old.promise : Promise.resolve(reply(value(), e))),
    }),
  );
  const reading = model.connect(account());
  await model.connect(account('2'));
  old.resolve(reply({ ...value(), settings: { autoPlay: true, rememberPosition: false } }));
  await reading;
  assert.equal(model.view.value?.settings.autoPlay, false);
  assert.equal(model.loading.value, false);
  const late = deferred<AccountData<StartupPreferencesView>>();
  const other = useStartupPreferences(backend({ startupPreferencesCommand: () => late.promise }));
  await other.connect(account());
  const writing = other.change('autoPlay', true);
  other.dispose();
  late.resolve(reply({ ...value(), settings: { autoPlay: true, rememberPosition: true } }));
  assert.equal(await writing, false);
  assert.equal(other.view.value?.settings.autoPlay, false);
});
test('corruption exposes only explicit repair and read failure leaves controls disabled', async () => {
  let corrupted = true,
    calls = 0,
    offline = false;
  const model = useStartupPreferences(
    backend({
      startupPreferences: async () => {
        if (offline) throw { message: 'storage unavailable', code: 'storage' };
        return reply(corrupted ? { ...value(), revision: null, damaged: true } : value());
      },
      startupPreferencesCommand: async (_, command) => {
        assert.equal(command.kind, 'repair');
        calls++;
        corrupted = false;
        return reply(value());
      },
    }),
  );
  await model.connect(account());
  assert.equal(model.canChange.value, false);
  assert.equal(await model.change('autoPlay', true), false);
  assert.equal(calls, 0);
  assert.equal(await model.repair(), true);
  assert.equal(model.canChange.value, true);
  assert.equal(await model.repair(), false);
  assert.equal(calls, 1);
  offline = true;
  await model.read();
  assert.equal(model.canChange.value, false);
  assert.ok(model.error.value);
});
