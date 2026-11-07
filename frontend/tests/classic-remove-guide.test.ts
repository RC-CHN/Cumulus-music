import assert from 'node:assert/strict';
import { test } from 'node:test';
import type { AccountView } from '../src/services/contracts.ts';
import {
  createClassicRemoveGuideScheduler,
  type ClassicRemoveGuide,
} from '../src/services/classicRemoveGuide.ts';

const expired: AccountView = {
  epoch: '7',
  status: 'expired',
  nickname: null,
  avatarUrl: null,
  classicRemoveGuide: 'synthetic-native-offer',
  loginNotice: { kind: 'reauthenticate', title: '请重新确认登录', message: '登录失效' },
};

function clock() {
  const queued: { run: () => void; delay: number; cancelled: boolean }[] = [];
  return {
    queued,
    schedule(run: () => void, delay: number) {
      const entry = { run, delay, cancelled: false };
      queued.push(entry);
      return () => {
        entry.cancelled = true;
      };
    },
  };
}

test('live classic offer delays exactly one open and duplicated account snapshots do not reopen', () => {
  const timer = clock();
  const opened: ClassicRemoveGuide[] = [];
  const dismissed: ClassicRemoveGuide[] = [];
  const lifecycle = createClassicRemoveGuideScheduler(
    () => true,
    (guide) => opened.push(guide),
    (guide) => dismissed.push(guide),
    timer,
  );
  lifecycle.observe(expired);
  lifecycle.observe({ ...expired });
  assert.deepEqual(opened, []);
  assert.equal(timer.queued.length, 1);
  assert.equal(timer.queued[0].delay, 1000);
  timer.queued[0].run();
  assert.deepEqual(opened, [
    {
      epoch: '7',
      ticket: 'synthetic-native-offer',
      title: '请重新确认登录',
      message: '登录失效',
    },
  ]);
  lifecycle.observe({ ...expired });
  assert.equal(timer.queued.length, 1);
  assert.deepEqual(dismissed, []);
});

test('old notices, a displaced account, background, and competing windows cannot auto-open', () => {
  const timer = clock();
  const discarded: string[] = [];
  let opens = 0;
  let visibleWithoutModal = true;
  const lifecycle = createClassicRemoveGuideScheduler(
    () => visibleWithoutModal,
    () => {
      opens++;
    },
    (guide) => discarded.push(guide.epoch),
    timer,
  );
  lifecycle.observe({ ...expired, classicRemoveGuide: undefined });
  lifecycle.observe({ ...expired, loginNotice: { kind: 'expired', title: null, message: '失效' } });
  assert.equal(timer.queued.length, 0);
  lifecycle.observe(expired);
  lifecycle.observe({ ...expired, epoch: '8', status: 'authenticated' });
  assert.ok(timer.queued[0].cancelled);
  timer.queued[0].run(); // A timeout already queued must also reject the displaced account.
  visibleWithoutModal = false;
  lifecycle.observe({ ...expired, epoch: '9', classicRemoveGuide: 'another-offer' });
  timer.queued[1].run();
  assert.deepEqual(discarded, ['7', '9']);
  assert.equal(opens, 0);
});
