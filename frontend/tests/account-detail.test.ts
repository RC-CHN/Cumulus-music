import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, ref } from 'vue';
import { useAccountRead } from '../src/composables/useAccountRead.ts';
import {
  accountDate,
  accountGender,
  accountRegion,
  bindingLabel,
  groupLoginDevices,
  loginDeviceIcon,
  type LoginDevice,
} from '../src/services/accountDetail.ts';

test('profile renders recovered region names without inventing unknown cities', () => {
  assert.equal(accountRegion(330000, 330100), '浙江省 · 杭州市');
  assert.equal(accountRegion(110000, 110100), '北京市');
  assert.equal(accountRegion(330000, 999999), '浙江省 · 城市代码 999999');
  assert.equal(accountRegion(null, null), '');
});
import type { AccountData } from '../src/services/contracts.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function fixture(read: (epoch: string) => Promise<AccountData<string[]>>) {
  const scope = effectScope();
  const owner = ref<string | null>('one');
  let expiry = 0;
  const model = scope.run(() =>
    useAccountRead(
      () => owner.value,
      read,
      (value) => value.length === 0,
      () => expiry++,
    ),
  )!;
  return { scope, owner, model, expiry: () => expiry };
}

const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

test('account reads distinguish empty success from a failed request and ignore duplicate refreshes', async () => {
  const pending = deferred<AccountData<string[]>>();
  let calls = 0;
  const f = fixture(async (epoch) => {
    calls++;
    return calls === 1 ? pending.promise : { epoch, value: [] };
  });
  assert.equal(f.model.state.value, 'loading');
  await f.model.refresh();
  assert.equal(calls, 1);
  pending.reject({ code: 'network', message: 'synthetic backend detail' });
  await settle();
  assert.equal(f.model.state.value, 'error');
  assert.equal(f.model.value.value, undefined);
  assert.doesNotMatch(f.model.error.value, /synthetic/);
  await f.model.refresh();
  assert.equal(f.model.state.value, 'empty');
  assert.deepEqual(f.model.value.value, []);
  assert.equal(calls, 2);
  f.scope.stop();
});

test('old account replies cannot replace a newer account, and mismatched epochs are refused', async () => {
  const old = deferred<AccountData<string[]>>();
  const current = deferred<AccountData<string[]>>();
  const f = fixture((epoch) => (epoch === 'one' ? old.promise : current.promise));
  f.owner.value = 'two';
  old.resolve({ epoch: 'one', value: ['old synthetic device'] });
  await settle();
  assert.equal(f.model.state.value, 'loading');
  assert.equal(f.model.value.value, undefined);
  current.resolve({ epoch: 'one', value: ['mismatched synthetic device'] });
  await settle();
  assert.equal(f.model.state.value, 'error');
  assert.equal(f.model.value.value, undefined);
  f.scope.stop();
});

test('sign-out and closing a page clear visible data without a late expiry notification', async () => {
  const late = deferred<AccountData<string[]>>();
  let calls = 0;
  const f = fixture(async (epoch) => {
    calls++;
    return calls === 1 ? { epoch, value: ['synthetic current device'] } : late.promise;
  });
  await settle();
  assert.equal(f.model.state.value, 'ready');
  assert.deepEqual(f.model.value.value, ['synthetic current device']);
  f.owner.value = null;
  assert.equal(f.model.state.value, 'signed-out');
  assert.equal(f.model.value.value, undefined);
  await f.model.refresh();
  assert.equal(calls, 1);
  f.owner.value = 'two';
  f.scope.stop();
  late.reject({ code: 'session_expired', message: 'synthetic expiry' });
  await settle();
  assert.equal(f.expiry(), 0);
  assert.equal(f.model.value.value, undefined);
});

test('active owner expiry is reported without retaining account data', async () => {
  const f = fixture(async () => {
    throw { code: 'session_expired', message: 'synthetic expiry' };
  });
  await settle();
  assert.equal(f.expiry(), 1);
  assert.equal(f.model.state.value, 'error');
  assert.equal(f.model.value.value, undefined);
  f.scope.stop();
});

test('device groups pin the current signed-in device without disturbing service order', () => {
  const devices: LoginDevice[] = [
    { id: 'a', current: false, signedIn: true },
    { id: 'b', current: false, signedIn: false },
    { id: 'c', current: true, signedIn: true },
    { id: 'd', current: false, signedIn: true },
    { id: 'e', current: false, signedIn: false },
  ];
  const groups = groupLoginDevices(devices);
  assert.deepEqual(
    groups.signedIn.map((device) => device.id),
    ['c', 'a', 'd'],
  );
  assert.deepEqual(
    groups.signedOut.map((device) => device.id),
    ['b', 'e'],
  );
  assert.deepEqual(
    devices.map((device) => device.id),
    ['a', 'b', 'c', 'd', 'e'],
  );
});

test('optional account fields do not acquire guessed dates, gender or provider labels', () => {
  assert.equal(accountDate(new Date(1969, 11, 15).getTime()), '1969.12.15');
  assert.equal(accountDate(new Date(2025, 3, 9, 7, 8, 9).getTime(), true), '2025.04.09 07:08:09');
  for (const value of [null, undefined, 0, Infinity, NaN]) assert.equal(accountDate(value), null);
  assert.equal(accountGender(0), '未提供');
  assert.equal(accountGender(null), '未提供');
  assert.equal(bindingLabel('unknown'), '其他绑定');
  assert.equal(loginDeviceIcon('Android'), 'phone');
});
