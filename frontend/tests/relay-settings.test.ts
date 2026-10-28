import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useRelay } from '../src/composables/useRelay.ts';
import type { RelayClient, RelayView, RelaySettingCommand } from '../src/services/relay.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'Synthetic',
  avatarUrl: null,
};
const page = (): RelayView => ({
  revision: 'a'.repeat(64),
  availability: 'available',
  setting: false,
  alert: null,
  operations: [],
  settings: { revision: 'b'.repeat(64), enabled: false, canChange: true, operations: [] },
});
const reply = (value: RelayView, epoch = '1') => ({ epoch, value: structuredClone(value) });
function backend(value: RelayView, write: RelayClient['relaySettingCommand']): RelayClient {
  return {
    relayView: async () => reply(value),
    relayCommand: async () => reply(value),
    relaySettingCommand: write,
  };
}
function operation(
  command: Extract<RelaySettingCommand, { enabled: boolean }>,
  phase: 'confirmed' | 'uncertain' | 'cancelled',
) {
  return {
    id: command.id,
    enabled: command.enabled,
    before: false,
    observed: phase === 'confirmed',
    phase,
    createdAt: 1,
    lastReadAt: phase === 'confirmed' ? 1 : null,
    readRevision: phase === 'confirmed' ? 'c'.repeat(64) : null,
  };
}
test('relay setting checkbox waits for confirmed native state and ignores duplicate clicks', async () => {
  const value = page();
  let resolve!: (value: ReturnType<typeof reply>) => void;
  let calls = 0;
  const model = useRelay(
    backend(value, async () => {
      calls++;
      return new Promise((r) => {
        resolve = r;
      });
    }),
  );
  model.connect(account);
  await model.read(true);
  const changing = model.changeSetting(true);
  assert.equal(model.page.value?.settings?.enabled, false);
  assert.equal(await model.changeSetting(true), false);
  assert.equal(calls, 1);
  value.settings!.enabled = true;
  resolve(reply(value));
  await changing;
  assert.equal(model.page.value?.settings?.enabled, true);
});
test('lost setting reply reads durable intent then performs only explicit readback', async () => {
  const value = page(),
    commands: string[] = [];
  const model = useRelay(
    backend(value, async (_, cmd) => {
      commands.push(cmd.kind);
      if (cmd.kind === 'setting_change') {
        value.settings!.operations = [operation(cmd, 'uncertain')];
        value.settings!.canChange = false;
        throw { code: 'network', message: 'lost' };
      }
      assert.equal(cmd.kind, 'setting_reconcile');
      value.settings!.enabled = true;
      value.settings!.operations[0].phase = 'confirmed';
      value.settings!.canChange = true;
      return reply(value);
    }),
  );
  model.connect(account);
  await model.read(true);
  await model.changeSetting(true);
  assert.equal(model.settingUnknown.value, undefined);
  assert.equal(model.canChangeSetting.value, false);
  await model.reconcileSetting(model.page.value!.settings!.operations[0].id);
  assert.deepEqual(commands, ['setting_change', 'setting_reconcile']);
  assert.equal(model.page.value?.settings?.enabled, true);
});
test('unseen setting invocation is cancelled before allowing another change', async () => {
  const value = page(),
    commands: string[] = [];
  const model = useRelay(
    backend(value, async (_, cmd) => {
      commands.push(cmd.kind);
      if (cmd.kind === 'setting_change') throw { code: 'network', message: 'lost' };
      assert.equal(cmd.kind, 'setting_cancel_unseen');
      if (cmd.kind === 'setting_cancel_unseen')
        value.settings!.operations = [operation(cmd, 'cancelled')];
      return reply(value);
    }),
  );
  model.connect(account);
  await model.read(true);
  await model.changeSetting(true);
  assert.ok(model.settingUnknown.value);
  await model.read(true);
  assert.equal(await model.changeSetting(true), false);
  await model.cancelUnknownSetting();
  assert.equal(model.settingUnknown.value, undefined);
  assert.deepEqual(commands, ['setting_change', 'setting_cancel_unseen']);
});
test('a late setting acknowledgement cannot repopulate a switched account', async () => {
  const value = page();
  let resolve!: (value: ReturnType<typeof reply>) => void;
  const model = useRelay(
    backend(
      value,
      async () =>
        new Promise((r) => {
          resolve = r;
        }),
    ),
  );
  model.connect(account);
  await model.read(true);
  const work = model.changeSetting(true);
  model.connect({ ...account, epoch: '2' });
  value.settings!.enabled = true;
  resolve(reply(value));
  await work;
  assert.equal(model.page.value, undefined);
  assert.equal(model.busy.value, false);
  assert.equal(model.settingUnknown.value, undefined);
});
