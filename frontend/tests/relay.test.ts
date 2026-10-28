import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useRelay } from '../src/composables/useRelay.ts';
import type { RelayClient, RelayView } from '../src/services/relay.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'Synthetic',
  avatarUrl: null,
};
const page = (): RelayView => ({
  revision: 'a'.repeat(64),
  availability: 'available',
  setting: true,
  alert: { title: 'Continue', deviceName: 'Fixture', resourceName: 'Song', buttonText: 'Continue' },
  operations: [],
});
const reply = (value = page(), epoch = '1') => ({ value: structuredClone(value), epoch });
const backend = (extra: Partial<RelayClient> = {}): RelayClient => ({
  relayView: async (e) => reply(page(), e),
  relayCommand: async (e) => reply(page(), e),
  ...extra,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
test('relay consumes only an explicit continuation and ignores duplicate clicks while pending', async () => {
  const gate = deferred<ReturnType<typeof reply>>();
  let calls = 0;
  const model = useRelay(
    backend({
      relayCommand: async () => {
        calls++;
        return gate.promise;
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  assert.equal(calls, 0);
  const operation = model.continueHere();
  assert.equal(model.alert.value, undefined);
  assert.equal(await model.continueHere(), false);
  assert.equal(calls, 1);
  gate.resolve(reply({ ...page(), alert: null }));
  await operation;
  assert.equal(model.busy.value, false);
});
test('relay lost IPC acknowledgement inspects native history without repeating the pull', async () => {
  let calls = 0,
    requestId = '';
  const value = page();
  const model = useRelay(
    backend({
      relayView: async () => reply(value),
      relayCommand: async (_, cmd) => {
        calls++;
        if ('id' in cmd) requestId = cmd.id;
        value.alert = null;
        value.operations = [
          { id: requestId, phase: 'installed', pullPhase: 'ready', canRetry: false, createdAt: 1 },
        ];
        throw { code: 'network', message: 'lost' };
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  await model.continueHere();
  assert.equal(calls, 1);
  assert.equal(model.unknown.value, undefined);
  assert.equal(model.page.value?.operations[0].phase, 'installed');
  assert.equal(await model.continueHere(), false);
});
test('relay unresolved host reply is cancelled before delayed IPC can execute', async () => {
  let calls = 0;
  const commands: string[] = [];
  const value = page();
  const model = useRelay(
    backend({
      relayView: async () => reply(value),
      relayCommand: async (_, cmd) => {
        calls++;
        commands.push(cmd.kind);
        if (cmd.kind === 'continue') throw { code: 'network', message: 'lost' };
        if (cmd.kind === 'cancel_unseen')
          value.operations = [
            {
              id: cmd.id,
              phase: 'cancelled',
              pullPhase: 'cancelled',
              canRetry: false,
              createdAt: 1,
            },
          ];
        return reply(value);
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  await model.continueHere();
  assert.ok(model.unknown.value);
  await model.continueHere();
  assert.equal(calls, 1);
  await model.cancelUnknown();
  assert.deepEqual(commands, ['continue', 'cancel_unseen']);
  assert.equal(model.unknown.value, undefined);
});
test('relay read failure retains display but blocks continue and account switch rejects late data', async () => {
  let offline = false;
  const gate = deferred<ReturnType<typeof reply>>();
  let held = false;
  const model = useRelay(
    backend({
      relayView: async () => {
        if (held) return gate.promise;
        if (offline) throw { code: 'network', message: 'offline' };
        return reply();
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  offline = true;
  await model.read(true);
  assert.ok(model.page.value?.alert);
  assert.equal(await model.continueHere(), false);
  held = true;
  const work = model.read(true);
  model.connect({ ...account, epoch: '2' });
  gate.resolve(reply());
  await work;
  assert.equal(model.page.value, undefined);
  assert.equal(model.loading.value, false);
});
test('relay expiration releases pending flags and terminal plans cannot be retried', async () => {
  let expired = 0;
  const model = useRelay(
    backend({
      relayCommand: async () => {
        throw { code: 'session_expired', message: 'expired' };
      },
    }),
    () => expired++,
  );
  model.connect(account);
  await model.read(true);
  assert.equal(await model.retry('unlisted'), false);
  await model.continueHere();
  assert.equal(expired, 1);
  assert.equal(model.busy.value, false);
  assert.equal(model.page.value, undefined);
  assert.equal(model.unknown.value, undefined);
});
