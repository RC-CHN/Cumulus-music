import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  createSecurityQrPoller,
  securityQrActions,
  type SecurityQrClock,
  type SecurityQrPollState,
} from '../src/services/securityQr.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function fixture(poll: (id: string) => Promise<boolean> = async () => true) {
  let state: SecurityQrPollState = {
    owner: 'login-one:qr-one',
    verificationId: 'qr-one',
    status: 'waiting_scan',
    loginStatus: 'security_qr',
    pollAfterMs: 1000,
    busy: false,
    active: true,
  };
  const jobs = new Set<{ callback: () => void; delay: number }>();
  const clock: SecurityQrClock = {
    schedule(callback, delay) {
      const job = { callback, delay };
      jobs.add(job);
      return () => jobs.delete(job);
    },
  };
  const calls: string[] = [];
  let failures = 0,
    refreshes = 0;
  const controller = createSecurityQrPoller({
    read: () => state,
    poll: async (id) => {
      calls.push(id);
      return poll(id);
    },
    failed: () => failures++,
    initialRefresh: () => refreshes++,
    clock,
  });
  function change(next: Partial<SecurityQrPollState>) {
    state = { ...state, ...next };
    controller.sync();
  }
  function fire() {
    const job = jobs.values().next().value;
    assert.ok(job, 'a native polling deadline must exist');
    jobs.delete(job);
    job.callback();
    return job;
  }
  return {
    controller,
    jobs,
    calls,
    change,
    fire,
    failures: () => failures,
    refreshes: () => refreshes,
  };
}
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));

test('a possibly acknowledged native poll permits only read-only reconciliation', () => {
  assert.deepEqual(
    securityQrActions({ status: 'error', canReconcile: true, canRefresh: false }, 'error'),
    { canRefresh: false, canReconcile: true },
  );
  assert.deepEqual(
    securityQrActions({ status: 'waiting_scan', canReconcile: true, canRefresh: true }, 'error'),
    { canRefresh: false, canReconcile: true },
  );
});

test('an explicit native 301 resolution enables refresh without starting a new QR automatically', () => {
  const f = fixture();
  f.change({ status: 'error', loginStatus: 'error', canReconcile: true });
  f.change({ canReconcile: false });
  assert.equal(f.jobs.size, 0);
  assert.equal(f.refreshes(), 0);
  assert.deepEqual(
    securityQrActions({ status: 'error', canReconcile: false, canRefresh: true }, 'error'),
    { canRefresh: true, canReconcile: false },
  );
  f.controller.dispose();
});

test('native refresh restrictions override local error and expiry fallback behavior', () => {
  for (const status of ['error', 'expired'] as const)
    assert.deepEqual(securityQrActions({ status, canRefresh: false }, 'error', true), {
      canRefresh: false,
      canReconcile: false,
    });
});

test('older QR fixtures retain authorized reconciliation and explicit error or expiry refresh', () => {
  assert.deepEqual(securityQrActions({ status: 'authorized' }, 'error'), {
    canRefresh: false,
    canReconcile: true,
  });
  for (const status of ['error', 'expired'] as const)
    assert.deepEqual(securityQrActions({ status }, 'security_qr'), {
      canRefresh: true,
      canReconcile: false,
    });
  assert.deepEqual(securityQrActions(null, 'security_qr'), {
    canRefresh: false,
    canReconcile: false,
  });
});

test('an existing verification ticket waits the native interval and does not refresh on mount', async () => {
  const f = fixture();
  f.controller.sync();
  assert.equal(f.calls.length, 0);
  assert.equal(f.refreshes(), 0);
  assert.equal(f.jobs.size, 1);
  assert.equal(f.fire().delay, 1000);
  assert.deepEqual(f.calls, ['qr-one']);
  await settle();
  assert.equal(f.jobs.size, 1);
  f.controller.dispose();
});

test('an initial idle ticket waits for busy to finish and attempts exactly one refresh', () => {
  const f = fixture();
  f.change({ status: 'idle', busy: true });
  assert.equal(f.refreshes(), 0);
  f.change({ busy: false });
  assert.equal(f.refreshes(), 1);
  f.controller.sync();
  f.change({ status: 'idle' });
  assert.equal(f.refreshes(), 1);
  assert.equal(f.jobs.size, 0);
  f.controller.dispose();
});

test('returning to idle after an existing ticket never triggers an automatic refresh', () => {
  const f = fixture();
  f.controller.sync();
  f.change({ status: 'idle' });
  assert.equal(f.refreshes(), 0);
  assert.equal(f.jobs.size, 0);
  f.controller.dispose();
});

test('only one poll may be pending and the next deadline starts after its completion', async () => {
  const pending = deferred<boolean>();
  const f = fixture(() => pending.promise);
  f.controller.sync();
  f.fire();
  f.controller.sync();
  f.change({ busy: true });
  f.change({ busy: false });
  assert.equal(f.calls.length, 1);
  assert.equal(f.jobs.size, 0);
  pending.resolve(true);
  await settle();
  assert.equal(f.jobs.size, 1);
  f.controller.dispose();
});

test('busy, captcha, native terminal states, an inert owner and invalid delays never poll', () => {
  const changes: Partial<SecurityQrPollState>[] = [
    { busy: true },
    { active: false },
    { canReconcile: true },
    { loginStatus: 'captcha' },
    { loginStatus: 'error' },
    { status: 'idle' },
    { status: 'creating' },
    { status: 'challenge' },
    { status: 'authorized' },
    { status: 'expired' },
    { status: 'error' },
    { status: 'cancelled' },
    { pollAfterMs: 0 },
    { pollAfterMs: Number.NaN },
  ];
  for (const change of changes) {
    const f = fixture();
    f.controller.sync();
    f.change(change);
    assert.equal(f.jobs.size, 0, JSON.stringify(change));
    assert.equal(f.calls.length, 0);
    f.controller.dispose();
  }
});

test('a cancelled deadline cannot poll a replacement verification ticket', () => {
  const f = fixture();
  f.controller.sync();
  const stale = f.jobs.values().next().value!;
  f.change({ owner: 'login-one:qr-two', verificationId: 'qr-two' });
  stale.callback();
  assert.equal(f.calls.length, 0);
  f.fire();
  assert.deepEqual(f.calls, ['qr-two']);
  f.controller.dispose();
});

test('a late cancelled callback cannot detach or fire the current deadline', () => {
  const f = fixture();
  f.controller.sync();
  const stale = f.jobs.values().next().value!;
  f.change({ busy: true });
  f.change({ busy: false });
  stale.callback();
  assert.equal(f.calls.length, 0);
  assert.equal(f.jobs.size, 1);
  f.controller.dispose();
  assert.equal(f.jobs.size, 0);
});

test('a late failure from an old ticket cannot stop the replacement ticket', async () => {
  const pending = deferred<boolean>();
  const f = fixture(() => pending.promise);
  f.controller.sync();
  f.fire();
  f.change({ owner: 'login-one:qr-two', verificationId: 'qr-two' });
  assert.equal(f.jobs.size, 0);
  pending.resolve(false);
  await settle();
  assert.equal(f.failures(), 0);
  assert.equal(f.jobs.size, 1);
  f.controller.dispose();
});

test('a failed poll stays stopped until an explicit refresh replaces or resumes it', async () => {
  const f = fixture(async () => false);
  f.controller.sync();
  f.fire();
  await settle();
  assert.equal(f.failures(), 1);
  f.controller.sync();
  f.change({ status: 'waiting_confirmation' });
  assert.equal(f.jobs.size, 0);
  assert.equal(f.calls.length, 1);
  f.controller.pause();
  f.controller.resume();
  assert.equal(f.jobs.size, 1);
  f.controller.dispose();
});

test('a thrown polling error also stops instead of silently replaying requests', async () => {
  const f = fixture(async () => {
    throw Error('synthetic transport failure');
  });
  f.controller.sync();
  f.fire();
  await settle();
  assert.equal(f.failures(), 1);
  assert.equal(f.jobs.size, 0);
  f.controller.dispose();
});

test('leaving while a poll is pending suppresses timers and late failure callbacks', async () => {
  const pending = deferred<boolean>();
  const f = fixture(() => pending.promise);
  f.controller.sync();
  f.fire();
  f.controller.dispose();
  pending.resolve(false);
  await settle();
  assert.equal(f.failures(), 0);
  assert.equal(f.jobs.size, 0);
  assert.equal(f.calls.length, 1);
});
