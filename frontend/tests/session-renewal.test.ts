import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSessionRenewal } from '../src/composables/useSessionRenewal.ts';
import {
  renewalMessage,
  type SessionRenewalView,
  type RenewalClient,
} from '../src/services/renewal.ts';
const view = (state: SessionRenewalView['state'] = 'ready'): SessionRenewalView => ({
  epoch: '1',
  state,
  busy: false,
  lastSuccess: null,
  canCheck: state === 'ready',
  canReconcile: state === 'recovery_required',
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function fixture(overrides: Partial<RenewalClient> = {}) {
  let event!: (next: SessionRenewalView) => void;
  const core: RenewalClient = {
    sessionRenewal: async () => view(),
    renewSession: async () => view('refreshed'),
    onSessionRenewal: async (handler) => {
      event = handler;
      return () => {};
    },
    ...overrides,
  };
  return { model: useSessionRenewal(core, '1'), event: (v: SessionRenewalView) => event(v) };
}
test('late snapshot cannot overwrite a newer foreground event or another account', async () => {
  const late = deferred<SessionRenewalView>();
  const f = fixture({ sessionRenewal: () => late.promise });
  const connected = f.model.connect();
  await Promise.resolve();
  f.event(view('refreshed'));
  f.event({ ...view('expired'), epoch: '2' });
  late.resolve(view());
  await connected;
  assert.equal(f.model.view.value?.state, 'refreshed');
  f.model.dispose();
  f.event(view('expired'));
  assert.equal(f.model.view.value?.state, 'refreshed');
});
test('lost write reply reads recovery state and next user action only reconciles', async () => {
  const actions: string[] = [];
  let count = 0;
  const f = fixture({
    sessionRenewal: async () => view(count++ === 0 ? 'ready' : 'recovery_required'),
    renewSession: async (_, action) => {
      actions.push(action);
      throw { message: '连接中断', code: 'network' };
    },
  });
  await f.model.connect();
  await f.model.run();
  assert.deepEqual(actions, ['check']);
  assert.equal(f.model.view.value?.state, 'recovery_required');
  await f.model.run();
  assert.deepEqual(actions, ['check', 'reconcile']);
});
test('duplicates and disposal never repeat a renewal or publish a late response', async () => {
  const late = deferred<SessionRenewalView>();
  let writes = 0,
    reads = 0;
  const f = fixture({
    sessionRenewal: async () => {
      reads++;
      return view();
    },
    renewSession: () => {
      writes++;
      return late.promise;
    },
  });
  await f.model.connect();
  const pending = f.model.run();
  await f.model.run();
  f.model.dispose();
  late.resolve(view('refreshed'));
  await pending;
  assert.equal(writes, 1);
  assert.equal(reads, 1);
  assert.equal(f.model.view.value?.state, 'ready');
});
test('missing credentials shows the limitation without offering a refresh write', async () => {
  const f = fixture({
    sessionRenewal: async () => view('missing_credentials'),
    renewSession: async () => {
      throw Error('must not send');
    },
  });
  await f.model.connect();
  await f.model.run();
  assert.match(renewalMessage(f.model.view.value!.state), /继续使用/);
  assert.equal(f.model.error.value, '');
});
test('subscription completed after dialog disposal is immediately released', async () => {
  const late = deferred<() => void>();
  let released = 0,
    reads = 0;
  const f = fixture({
    onSessionRenewal: () => late.promise,
    sessionRenewal: async () => {
      reads++;
      return view();
    },
  });
  const pending = f.model.connect();
  f.model.dispose();
  late.resolve(() => released++);
  await pending;
  assert.equal(released, 1);
  assert.equal(reads, 0);
});
