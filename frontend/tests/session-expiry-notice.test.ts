import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { AccountView } from '../src/services/contracts.ts';
import { createSessionExpiryNotice } from '../src/services/sessionExpiryNotice.ts';

const signed: AccountView = {
  epoch: '3',
  status: 'authenticated',
  nickname: '合成账号',
  avatarUrl: null,
};
const expired: AccountView = {
  epoch: '4',
  status: 'expired',
  nickname: null,
  avatarUrl: null,
  liveJskExpiry: true,
  loginNotice: { kind: 'expired', title: null, message: '合成会话已经失效' },
};

function setup() {
  const seen: (string | undefined)[] = [];
  const timers: { run: () => void; delay: number; cancelled: boolean }[] = [];
  const controller = createSessionExpiryNotice((message) => seen.push(message), {
    schedule(run, delay) {
      const timer = { run, delay, cancelled: false };
      timers.push(timer);
      return () => {
        timer.cancelled = true;
      };
    },
  });
  return { controller, seen, timers };
}

test('live ordinary JSK invalidation announces once and expires after seven seconds', () => {
  const { controller, seen, timers } = setup();
  controller.observe(expired, signed);
  controller.observe({ ...expired }, expired);
  assert.deepEqual(seen, ['合成会话已经失效']);
  assert.equal(timers.length, 1);
  assert.equal(timers[0].delay, 7000);
  timers[0].run();
  controller.observe(expired, signed);
  assert.deepEqual(seen, ['合成会话已经失效', undefined]);
  assert.equal(timers.length, 1);
});

test('cold or legacy notices and explicit reauthentication never open the transient notice', () => {
  const { controller, seen, timers } = setup();
  controller.observe(expired, { ...signed, status: 'unknown' });
  controller.observe({ ...expired, liveJskExpiry: undefined }, signed);
  controller.observe(
    { ...expired, loginNotice: { kind: 'reauthenticate', title: null, message: '请重新登录' } },
    signed,
  );
  assert.deepEqual(seen, []);
  assert.equal(timers.length, 0);
});

test('switching accounts retires the notice and an old timeout cannot hide a replacement', () => {
  const { controller, seen, timers } = setup();
  controller.observe(expired, signed);
  const replacement = { ...signed, epoch: '5' };
  controller.observe(replacement, expired);
  assert.equal(timers[0].cancelled, true);
  controller.observe({ ...expired, epoch: '6' }, replacement);
  timers[0].run();
  assert.deepEqual(seen, ['合成会话已经失效', undefined, '合成会话已经失效']);
  controller.dispose();
  assert.equal(timers[1].cancelled, true);
  assert.equal(seen.at(-1), undefined);
});
