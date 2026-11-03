import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useServerQuickLogin } from '../src/composables/useServerQuickLogin.ts';
import type {
  ServerQuickLoginClient,
  ServerQuickLoginPhase,
} from '../src/services/serverQuickLogin.ts';

const epoch = 'guest';
const result = (phase: ServerQuickLoginPhase, canReconcile = false) => ({
  epoch,
  value: { operationId: 'quick-id', phase, canReconcile, status: null },
});

test('quick login requires guest consent and uncertainty only permits a read', async () => {
  let current: string | null = null;
  const calls: string[] = [];
  let adopted = 0;
  const client: ServerQuickLoginClient = {
    beginServerQuickLogin: async (owner, savedId, consent) => {
      assert.deepEqual([owner, savedId, consent], [epoch, 'saved-id', true]);
      calls.push('begin');
      return result('ready');
    },
    serverQuickLoginCommand: async (owner, operationId, command) => {
      assert.deepEqual([owner, operationId], [epoch, 'quick-id']);
      calls.push(command);
      if (command === 'execute') throw Error('synthetic transport loss');
      return result('authenticated');
    },
  };
  const model = useServerQuickLogin(
    client,
    () => current,
    () => adopted++,
  );
  await model.start('saved-id', true);
  current = epoch;
  await model.start('saved-id', false);
  assert.deepEqual(calls, []);
  await model.start('saved-id', true);
  assert.equal(model.problem.value, 'read_required');
  assert.equal(adopted, 0);
  await model.start('saved-id', true);
  await model.reconcile();
  await model.reconcile();
  assert.deepEqual(calls, ['begin', 'execute', 'reconcile']);
  assert.equal(adopted, 1);
  model.dispose();
});

test('cancel during preparation disposes a late ready result without exchanging', async () => {
  const calls: string[] = [];
  let prepared!: (value: ReturnType<typeof result>) => void;
  const model = useServerQuickLogin(
    {
      beginServerQuickLogin: () => new Promise((resolve) => (prepared = resolve)),
      serverQuickLoginCommand: async (_owner, _operationId, command) => {
        calls.push(command);
        return result('cancelled');
      },
    },
    () => epoch,
    () => assert.fail('cancelled quick login published authorization'),
  );
  const pending = model.start('saved-id', true);
  assert.equal(await model.cancel(), true);
  prepared(result('ready'));
  await pending;
  assert.deepEqual(calls, ['cancel']);
  assert.equal(model.view.value, undefined);
});

test('verification-required is terminal for this exchange and leaves only cancellation', async () => {
  const calls: string[] = [];
  const model = useServerQuickLogin(
    {
      beginServerQuickLogin: async () => result('ready'),
      serverQuickLoginCommand: async (_owner, _operationId, command) => {
        calls.push(command);
        return result(command === 'execute' ? 'verification_required' : 'cancelled');
      },
    },
    () => epoch,
    () => assert.fail('verification has not completed'),
  );
  await model.start('saved-id', true);
  assert.equal(model.view.value?.phase, 'verification_required');
  assert.equal(model.canRead.value, false);
  await model.reconcile();
  await model.start('saved-id', true);
  assert.equal(await model.cancel(), true);
  assert.deepEqual(calls, ['execute', 'cancel']);
});
