import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useLoginDeviceRemoval } from '../src/composables/useLoginDeviceRemoval.ts';
import type {
  LoginDeviceRemovalClient,
  LoginDeviceRemovalView,
} from '../src/services/loginDeviceRemoval.ts';

const id = 'a'.repeat(32);
const initial: LoginDeviceRemovalView = {
  id,
  phase: 'needs_sms',
  phoneTail: '0123',
  resendAfterSeconds: 0,
  smsNotAccepted: false,
  observedPresent: null,
};
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));
function fixture(epoch: string, client: LoginDeviceRemovalClient) {
  const scope = effectScope();
  const owner = ref<string | null>(epoch);
  let absent = 0;
  const model = scope.run(() =>
    useLoginDeviceRemoval(
      client,
      () => owner.value,
      () => absent++,
      () => {},
    ),
  )!;
  return { scope, owner, model, absent: () => absent };
}

test('recovering an SMS challenge does not send a message and the native cooldown prevents another send', async () => {
  const commands: string[] = [];
  const f = fixture('cooldown', {
    accountLoginDeviceRemoval: async (epoch) => ({ epoch, value: initial }),
    accountLoginDeviceRemovalCommand: async (epoch, command) => {
      commands.push(command.kind);
      return { epoch, value: { ...initial, phase: 'sms_send_uncertain', resendAfterSeconds: 60 } };
    },
  });
  await tick();
  assert.deepEqual(commands, []);
  assert.equal(f.model.canBegin.value, false);
  await f.model.command({ kind: 'submit_sms', operation_id: id, code: '123456' });
  await f.model.command({ kind: 'send_sms', operation_id: id });
  assert.equal(f.model.seconds.value, 60);
  await f.model.command({ kind: 'send_sms', operation_id: id });
  await f.model.command({ kind: 'submit_sms', operation_id: id, code: '12ab' });
  assert.deepEqual(commands, ['send_sms']);
  f.scope.stop();
});

test('lost removal receipt stays blocked across reopening and only absence from a fresh read resolves it', async () => {
  let status: LoginDeviceRemovalView | null = null;
  let visible = false;
  const actions: string[] = [];
  const client: LoginDeviceRemovalClient = {
    accountLoginDeviceRemoval: async (epoch) => ({ epoch, value: visible ? status : null }),
    accountLoginDeviceRemovalCommand: async (epoch, command) => {
      actions.push(command.kind);
      if (command.kind === 'begin') {
        status = { ...initial, phase: 'recovery_required' };
        throw new Error('synthetic lost receipt');
      }
      assert.equal(command.kind, 'reconcile');
      status = status?.observedPresent
        ? { ...initial, phase: 'observed_absent', observedPresent: false }
        : { ...initial, phase: 'recovery_required', observedPresent: true };
      return { epoch, value: status };
    },
  };
  const first = fixture('lost', client);
  await tick();
  await first.model.command({ kind: 'begin', device_id: id });
  assert.equal(first.model.readRequired.value, true);
  first.scope.stop();

  const next = fixture('lost', client);
  await tick();
  assert.equal(next.model.unseen.value, id);
  await next.model.command({ kind: 'begin', device_id: id });
  assert.deepEqual(actions, ['begin']);
  visible = true;
  await next.model.read();
  await next.model.command({ kind: 'reconcile', operation_id: id });
  assert.equal(next.model.view.value?.observedPresent, true);
  assert.equal(next.absent(), 0);
  await next.model.command({ kind: 'cancel', operation_id: id });
  await next.model.command({ kind: 'reconcile', operation_id: id });
  assert.equal(next.absent(), 1);
  assert.deepEqual(actions, ['begin', 'reconcile', 'reconcile']);
  next.scope.stop();
});

test('a result from the previous account cannot publish the removal notice', async () => {
  let complete!: (value: { epoch: string; value: LoginDeviceRemovalView }) => void;
  const f = fixture('previous', {
    accountLoginDeviceRemoval: async (epoch) => ({ epoch, value: null }),
    accountLoginDeviceRemovalCommand: () => new Promise((resolve) => (complete = resolve)),
  });
  await tick();
  const result = f.model.command({ kind: 'begin', device_id: id });
  f.owner.value = 'current';
  complete({
    epoch: 'previous',
    value: { ...initial, phase: 'observed_absent', observedPresent: false },
  });
  await result;
  await tick();
  assert.equal(f.absent(), 0);
  assert.equal(f.model.view.value, undefined);
  assert.equal(f.model.canBegin.value, true);
  f.scope.stop();
});
