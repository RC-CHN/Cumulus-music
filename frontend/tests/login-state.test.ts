import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useLogin } from '../src/composables/useLogin.ts';
import { loginMessage, type AuthClient, type LoginView } from '../src/services/auth.ts';
function state(id = 'one', status: LoginView['status'] = 'ready'): LoginView {
  return {
    operationId: id,
    mode: 'email',
    status,
    code: null,
    busy: false,
    captchaAvailable: status === 'captcha',
    retryAfterMs: 0,
    interrupted: false,
    challenge: {
      methods: [],
      nickname: null,
      avatarUrl: null,
      phoneMask: null,
      question: null,
      answers: [],
      reuseConfirmation: false,
    },
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function fixture(overrides: Partial<AuthClient> = {}) {
  const cancelled: string[] = [];
  let authorized = 0;
  const core: AuthClient = {
    sessionRenewal: async () => {
      throw Error('unused renewal');
    },
    renewSession: async () => {
      throw Error('unused renewal');
    },
    onSessionRenewal: async () => () => {},
    solveLoginCaptcha: async () => ({ kind: 'credentials', value: state('one', 'authenticated') }),
    onAccountChanged: async () => () => {},
    beginLogin: async () => state(),
    loginSnapshot: async () => state(),
    loginCommand: async () => state(),
    cancelLogin: async (id) => {
      cancelled.push(id);
    },
    savedAccounts: async (epoch) => ({ epoch, value: [] }),
    removeSavedAccount: async () => {
      throw Error('not expected');
    },
    logoutAccount: async () => {
      throw Error('not expected');
    },
    ...overrides,
  };
  return { model: useLogin(core, () => authorized++), cancelled, authorized: () => authorized };
}
test('closing during native login creation cancels the late candidate without displaying it', async () => {
  const d = deferred<LoginView>();
  const f = fixture({ beginLogin: () => d.promise });
  const started = f.model.start('1', 'email');
  f.model.dispose();
  d.resolve(state('late'));
  await started;
  assert.equal(f.model.view.value, undefined);
  assert.deepEqual(f.cancelled, ['late']);
});
test('an old authorized response cannot replace a newer login or notify its parent', async () => {
  const d = deferred<LoginView>();
  let created = 0;
  const f = fixture({
    beginLogin: async () => state(String(++created)),
    loginCommand: () => d.promise,
  });
  await f.model.start('1', 'email');
  const submitted = f.model.run({ kind: 'reconcile' });
  await f.model.start('2', 'email');
  d.resolve(state('1', 'authenticated'));
  await submitted;
  assert.equal(f.model.view.value?.operationId, '2');
  assert.equal(f.authorized(), 0);
  assert.deepEqual(f.cancelled, ['1']);
  f.model.dispose();
});
test('failed submission reads status and never automatically replays credentials or a send', async () => {
  let commands = 0,
    reads = 0;
  const f = fixture({
    loginCommand: async () => {
      commands++;
      throw { code: 'network', message: '网络请求失败' };
    },
    loginSnapshot: async () => {
      reads++;
      return { ...state('one', 'error'), interrupted: true, retryAfterMs: 59900 };
    },
  });
  await f.model.start('1', 'phone_sms');
  await f.model.run({ kind: 'send_sms', phone: '13800138000', country_code: '86' });
  assert.equal(commands, 1);
  assert.equal(reads, 1);
  assert.equal(f.model.seconds.value, 60);
  assert.equal(f.model.view.value?.interrupted, true);
  assert.match(f.model.error.value, /网络/);
  f.model.dispose();
});
test('native authenticated snapshot recovers a committed login after the command reply failed', async () => {
  const f = fixture({
    loginCommand: async () => {
      throw { code: 'network', message: '网络请求失败' };
    },
    loginSnapshot: async () => state('one', 'authenticated'),
  });
  await f.model.start('1', 'email');
  await f.model.run({ kind: 'reconcile' });
  assert.equal(f.authorized(), 1);
  f.model.dispose();
});
test('disposed login drops late status reads and preserves single submission', async () => {
  const d = deferred<LoginView>();
  let calls = 0;
  const f = fixture({
    loginCommand: async () => {
      calls++;
      throw Error('lost');
    },
    loginSnapshot: () => d.promise,
  });
  await f.model.start('1', 'email');
  const work = f.model.run({ kind: 'reconcile' });
  await Promise.resolve();
  await f.model.run({ kind: 'reconcile' });
  f.model.dispose();
  d.resolve(state('one', 'authenticated'));
  await work;
  assert.equal(calls, 1);
  assert.equal(f.authorized(), 0);
});
test('unintegrated security challenges and unconfirmed identity have explicit status text', () => {
  assert.match(loginMessage(state('one', 'captcha')), /打开安全验证/);
  assert.match(loginMessage(state('one', 'verification')), /尚未接入/);
  assert.match(loginMessage(state('one', 'authorized')), /核对/);
  assert.match(loginMessage(state('one', 'error')), /未确认/);
});

test('CAPTCHA uses the native window and ignores completion after login was replaced', async () => {
  const d = deferred<{ kind: 'credentials'; value: LoginView }>();
  let opened = 0,
    created = 0;
  const f = fixture({
    beginLogin: async () => state(String(++created), 'captcha'),
    solveLoginCaptcha: () => {
      opened++;
      return d.promise;
    },
  });
  await f.model.start('1', 'email');
  const pending = f.model.captcha();
  await f.model.captcha();
  assert.equal(opened, 1);
  await f.model.start('1', 'email');
  d.resolve({ kind: 'credentials', value: state('1', 'authenticated') });
  await pending;
  assert.equal(f.model.view.value?.operationId, '2');
  assert.equal(f.authorized(), 0);
  f.model.dispose();
});
test('closed CAPTCHA leaves challenge retryable without resending credentials', async () => {
  let sent = 0;
  const f = fixture({
    beginLogin: async () => state('one', 'captcha'),
    solveLoginCaptcha: async () => {
      throw { code: 'stale', message: '已取消' };
    },
    loginSnapshot: async () => state('one', 'captcha'),
    loginCommand: async () => {
      sent++;
      return state();
    },
  });
  await f.model.start('1', 'email');
  await f.model.captcha();
  assert.equal(f.model.view.value?.status, 'captcha');
  assert.equal(f.model.busy.value, false);
  assert.equal(sent, 0);
  f.model.dispose();
});
