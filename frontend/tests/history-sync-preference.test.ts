import assert from 'node:assert/strict';
import test from 'node:test';
import { effectScope, ref } from 'vue';
import { useHistorySyncPreference } from '../src/composables/useHistorySyncPreference.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type {
  HistorySyncAction,
  HistorySyncClient,
  HistorySyncState,
} from '../src/services/history.ts';

type Reply = AccountData<HistorySyncState>;
function deferred<T>() {
  let finish!: (value: T) => void;
  const promise = new Promise<T>((resolve) => (finish = resolve));
  return { promise, finish };
}
const flush = () => new Promise<void>((resolve) => queueMicrotask(resolve));

function fixture() {
  const owner = ref<string | null>('1');
  const scope = effectScope();
  const requests: { epoch: string; action: HistorySyncAction; finish: (reply: Reply) => void }[] =
    [];
  let observe!: (change: Reply) => void;
  let unlistens = 0;
  const subscription = deferred<() => void>();
  const core: HistorySyncClient = {
    onHistorySyncChanged(handler) {
      observe = handler;
      return subscription.promise;
    },
    historySyncPreference(epoch, action) {
      const request = deferred<Reply>();
      requests.push({ epoch, action, finish: request.finish });
      return request.promise;
    },
  };
  const model = scope.run(() => useHistorySyncPreference(core, () => owner.value, assert.fail))!;
  const snapshot = (epoch: string, revision: string, enabled: boolean, confirmed = false) => ({
    epoch,
    value: { revision, enabled, firstDisableConfirmed: confirmed },
  });
  return {
    owner,
    scope,
    model,
    requests,
    subscription,
    snapshot,
    observe,
    get unlistens() {
      return unlistens;
    },
    subscribe: () => subscription.finish(() => unlistens++),
  };
}

test('native experiment changes retain the confirmation draft and never submit it', async () => {
  const f = fixture();
  assert.equal(f.requests.length, 0);
  f.subscribe();
  await flush();
  f.requests[0].finish(f.snapshot('1', '1', true));
  await flush();
  assert.equal(f.model.prepareChange(), undefined);
  assert.equal(f.model.confirmOff.value, true);

  const read = f.model.refresh();
  f.observe(f.snapshot('1', '2', false));
  f.requests[1].finish(f.snapshot('1', '1', true));
  await read;
  assert.equal(f.model.value.value?.enabled, false);
  assert.equal(f.model.value.value?.firstDisableConfirmed, false);
  assert.equal(f.model.confirmOff.value, true);
  assert.equal(f.model.saved.value, false);
  assert.deepEqual(
    f.requests.map((request) => request.action),
    [{ kind: 'read' }, { kind: 'read' }],
  );

  const save = f.model.save(false, true);
  assert.deepEqual(f.requests[2].action, {
    kind: 'set',
    enabled: false,
    confirmed_first_disable: true,
  });
  f.observe(f.snapshot('1', '3', false, true));
  f.observe(f.snapshot('1', '2', true));
  f.requests[2].finish(f.snapshot('1', '3', false, true));
  await save;
  assert.equal(f.model.value.value?.enabled, false);
  assert.equal(f.model.value.value?.firstDisableConfirmed, true);
  assert.equal(f.model.confirmOff.value, false);
  assert.equal(f.model.saved.value, true);
  f.scope.stop();
  assert.equal(f.unlistens, 1);
});

test('new-account events win over old snapshots; disposed subscriptions are cleaned up', async () => {
  const f = fixture();
  f.subscribe();
  await flush();
  f.owner.value = '2';
  assert.equal(f.requests[1].epoch, '2');
  f.observe(f.snapshot('2', '5', false));
  f.observe(f.snapshot('1', '99', true, true));
  f.requests[0].finish(f.snapshot('1', '100', true));
  f.requests[1].finish(f.snapshot('2', '4', true));
  await flush();
  assert.equal(f.model.value.value?.revision, '5');
  assert.equal(f.model.value.value?.enabled, false);
  f.scope.stop();
  f.observe(f.snapshot('2', '6', true));
  assert.equal(f.model.value.value?.revision, '5');

  const late = fixture();
  late.scope.stop();
  late.subscribe();
  await flush();
  assert.equal(late.unlistens, 1);
  assert.equal(late.requests.length, 0);
});
