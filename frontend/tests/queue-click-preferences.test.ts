import assert from 'node:assert/strict';
import test from 'node:test';
import { useQueueClickPreferences } from '../src/composables/useQueueClickPreferences.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type {
  QueueClickPreferencesCommand,
  QueueClickPreferencesView,
} from '../src/services/queueClickPreferences.ts';

const account: AccountView = {
  epoch: '1',
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
};
const defaults: QueueClickPreferencesView = { revision: '0', damaged: false, replaceQueue: true };
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

test('queue click setting stays confirmed until native reply and serializes clicks', async () => {
  const command = deferred<QueueClickPreferencesView>();
  const calls: QueueClickPreferencesCommand[] = [];
  const model = useQueueClickPreferences({
    queueClickPreferences: async (epoch) => ({ epoch, value: defaults }),
    queueClickPreferencesCommand: async (epoch, input) => {
      calls.push(input);
      return { epoch, value: await command.promise };
    },
  });
  await model.connect(account);
  const changing = model.change(false);
  assert.equal(model.view.value?.replaceQueue, true);
  assert.equal(await model.change(false), false);
  command.resolve({ revision: '1', damaged: false, replaceQueue: false });
  assert.equal(await changing, true);
  assert.equal(model.view.value?.replaceQueue, false);
  assert.deepEqual(calls, [{ kind: 'change', expected_revision: '0', replace_queue: false }]);
});

test('a lost queue setting acknowledgement only reads back and requires refresh', async () => {
  let current = defaults;
  let reads = 0;
  let writes = 0;
  const model = useQueueClickPreferences({
    queueClickPreferences: async (epoch) => {
      reads++;
      return { epoch, value: current };
    },
    queueClickPreferencesCommand: async () => {
      writes++;
      current = { revision: '1', damaged: false, replaceQueue: false };
      throw { code: 'transport', message: '结果未确认' };
    },
  });
  await model.connect(account);
  assert.equal(await model.change(false), false);
  assert.equal(model.view.value?.replaceQueue, false);
  assert.equal(model.saved.value, false);
  assert.equal(model.canChange.value, false);
  assert.equal(reads, 2);
  assert.equal(writes, 1);
  await model.read();
  assert.equal(model.canChange.value, true);
});

test('a late queue setting reply cannot overwrite another account epoch', async () => {
  const pending = deferred<QueueClickPreferencesView>();
  const model = useQueueClickPreferences({
    queueClickPreferences: async (epoch) => ({ epoch, value: defaults }),
    queueClickPreferencesCommand: async (epoch) => ({ epoch, value: await pending.promise }),
  });
  await model.connect(account);
  const changing = model.change(false);
  await model.connect({ ...account, epoch: '2' });
  pending.resolve({ revision: '1', damaged: false, replaceQueue: false });
  assert.equal(await changing, false);
  assert.deepEqual(model.view.value, defaults);
});
