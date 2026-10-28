import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useLikeRecovery } from '../src/composables/useLikeRecovery.ts';
import type { AccountView, AccountData } from '../src/services/contracts.ts';
import type {
  LikeOperation,
  LikeRecoveryClient,
  LikeRecoveryView,
} from '../src/services/likeRecovery.ts';
const account = (epoch = '1'): AccountView => ({
  epoch,
  status: 'authenticated',
  nickname: 'synthetic',
  avatarUrl: null,
});
const row: LikeOperation = {
  id: '2',
  revision: 'a'.repeat(64),
  desired: true,
  observed: false,
  phase: 'uncertain',
  canAccept: true,
};
const page: LikeRecoveryView = { total: 1, offset: 0, operations: [row] };
const empty: LikeRecoveryView = { total: 0, offset: 0, operations: [] };
const turn = () => new Promise((resolve) => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (v: T) => void, reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
test('a lost recovery acknowledgement only inspects state, never replays the command', async () => {
  let calls = 0,
    resolved = false,
    reads = 0;
  const core: LikeRecoveryClient = {
    likeRecovery: async (epoch) => {
      reads++;
      return { epoch, value: resolved ? empty : page };
    },
    recoverLike: async () => {
      calls++;
      resolved = true;
      throw { code: 'transport', message: 'lost response' };
    },
  };
  const model = useLikeRecovery(core);
  model.connect(account());
  await turn();
  await model.command({ kind: 'accept_current', id: '2', revision: row.revision, observed: false });
  assert.equal(calls, 1);
  assert.equal(reads, 2);
  assert.equal(model.view.value.total, 0);
  assert.match(model.error.value, /lost response/);
  assert.equal(model.busy.value, false);
});
test('old account reads and command errors cannot expire or replace the new account', async () => {
  const old = deferred<AccountData<LikeOperation>>();
  let expired = 0;
  const core: LikeRecoveryClient = {
    likeRecovery: async (epoch) => ({ epoch, value: epoch === '1' ? page : empty }),
    recoverLike: () => old.promise,
  };
  const model = useLikeRecovery(core, () => expired++);
  model.connect(account());
  await turn();
  const task = model.command({ kind: 'reconcile', id: '2', revision: row.revision });
  model.connect(account('2'));
  await turn();
  old.reject({ code: 'session_expired', message: 'old session' });
  await task;
  assert.equal(expired, 0);
  assert.equal(model.error.value, '');
  assert.equal(model.view.value.total, 0);
  assert.equal(model.busy.value, false);
});
test('native change during inspection queues one fresh local read and disposal ignores replies', async () => {
  const first = deferred<AccountData<LikeRecoveryView>>();
  let reads = 0;
  const model = useLikeRecovery({
    likeRecovery: async (epoch) => (++reads === 1 ? first.promise : { epoch, value: empty }),
    recoverLike: async (epoch) => ({ epoch, value: row }),
  });
  model.connect(account());
  void model.refresh();
  void model.refresh();
  first.resolve({ epoch: '1', value: page });
  await turn();
  assert.equal(reads, 2);
  assert.equal(model.view.value.total, 0);
  const late = deferred<AccountData<LikeRecoveryView>>();
  const disposed = useLikeRecovery({
    likeRecovery: () => late.promise,
    recoverLike: async (epoch) => ({ epoch, value: row }),
  });
  disposed.connect(account());
  disposed.dispose();
  late.resolve({ epoch: '1', value: page });
  await turn();
  assert.equal(disposed.view.value.total, 0);
});

test('a new pending write cannot inherit the previous recovery success notice', async () => {
  const model = useLikeRecovery({
    likeRecovery: async (epoch) => ({ epoch, value: page }),
    recoverLike: async (epoch) => ({ epoch, value: { ...row, phase: 'confirmed' } }),
  });
  model.connect(account());
  await turn();
  await model.command({ kind: 'reconcile', id: '2', revision: row.revision });
  assert.match(model.notice.value, /已确认/);
  await model.refresh();
  assert.equal(model.notice.value, '');
  assert.equal(model.view.value.total, 1);
});
