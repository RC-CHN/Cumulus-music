import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useTogether } from '../src/composables/useTogether.ts';
import type { TogetherClient, TogetherView } from '../src/services/together.ts';
import type { AccountData } from '../src/services/contracts.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
function value(): TogetherView {
  return {
    room: null,
    tracks: [],
    mode: 'ORDER_LOOP',
    targetId: null,
    progressMs: null,
    playing: false,
    attached: false,
    realtimeReady: false,
    connection: { state: 'unavailable', message: null, received: 0, attempt: 0 },
    pending: [],
    syncError: null,
    checkedAtMs: null,
    localRecovery: false,
    ending: false,
    lifecycleNotice: null,
    sharedPhase: null,
    nativeTargetId: null,
    commonAccess: [],
  };
}
function client(extra: Partial<TogetherClient> = {}): TogetherClient {
  return {
    togetherView: async (epoch) => ({ epoch, value: value() }),
    togetherCommand: async (epoch) => ({ epoch, value: value() }),
    ...extra,
  };
}
function held<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
test('message connection failures do not retry writes until explicit reconnect', async () => {
  let calls = 0;
  const failed = value();
  failed.room = { id: 'room', role: 'waiting', members: [] };
  failed.attached = true;
  failed.connection = { state: 'failed', message: '消息连接失败', received: 1, attempt: 4 };
  const m = useTogether(
    client({
      togetherView: async (epoch) => ({ epoch, value: failed }),
      togetherCommand: async (epoch, command) => {
        calls++;
        assert.equal(command.kind, 'reconnect');
        return {
          epoch,
          value: {
            ...failed,
            connection: { state: 'connecting', message: null, received: 0, attempt: 1 },
          },
        };
      },
    }),
  );
  m.connect(account());
  await m.load(false);
  assert.equal(m.view.value?.connection.state, 'failed');
  assert.equal(calls, 0);
  await m.command({ kind: 'reconnect', room_id: 'room' });
  assert.equal(calls, 1);
  assert.equal(m.view.value?.connection.state, 'connecting');
  m.dispose();
});
test('together account generations discard late rooms and command results', async () => {
  const read = held<AccountData<TogetherView>>(),
    write = held<AccountData<TogetherView>>();
  const m = useTogether(
    client({
      togetherView: async (e) => (e === '1' ? read.promise : { epoch: e, value: value() }),
      togetherCommand: () => write.promise,
    }),
  );
  m.connect(account());
  const request = m.command({ kind: 'create' });
  m.connect(account('2'));
  const old = value();
  old.room = { id: 'private-old-room', role: 'waiting', members: [] };
  read.resolve({ epoch: '1', value: old });
  write.resolve({ epoch: '1', value: old });
  await request;
  await Promise.resolve();
  assert.equal(m.epoch.value, '2');
  assert.equal(m.view.value?.room, null);
  assert.equal(m.busy.value, false);
  m.dispose();
});
test('unknown write is read from disk and is never automatically repeated', async () => {
  let writes = 0;
  let dirty = false;
  const m = useTogether(
    client({
      togetherCommand: async () => {
        writes++;
        dirty = true;
        throw { code: 'network', message: 'offline' };
      },
      togetherView: async (epoch) => {
        const v = value();
        if (dirty)
          v.pending = [{ id: 'op', action: 'play', phase: 'uncertain', acknowledged: false }];
        return { epoch, value: v };
      },
    }),
  );
  m.connect(account());
  await m.load(false);
  await m.command({ kind: 'create' });
  assert.equal(m.view.value?.pending.length, 1);
  assert.equal(m.error.value, 'offline');
  await m.command({ kind: 'create' });
  await m.command({ kind: 'publish', room_id: 'room' });
  assert.equal(writes, 1);
  m.dispose();
});
test('refresh failure still exposes persisted recovery and unresolved operations', async () => {
  const m = useTogether(
    client({
      togetherView: async (epoch, refresh) => {
        if (refresh) throw { code: 'network', message: 'offline' };
        const v = value();
        v.localRecovery = true;
        v.pending = [{ id: 'old-end', action: 'end', phase: 'sending', acknowledged: false }];
        return { epoch, value: v };
      },
    }),
  );
  m.connect(account());
  await m.load(true);
  assert.equal(m.view.value?.localRecovery, true);
  assert.equal(m.view.value?.pending[0]?.action, 'end');
  assert.equal(m.loading.value, false);
  m.dispose();
});
test('a failed background response cannot replace a newer explicit operation result', async () => {
  const delayed = held<AccountData<TogetherView>>();
  let slow = false;
  const m = useTogether(
    client({
      togetherView: async (e) => (slow ? delayed.promise : { epoch: e, value: value() }),
      togetherCommand: async (epoch) => {
        const v = value();
        v.room = { id: 'new', role: 'waiting', members: [] };
        return { epoch, value: v };
      },
    }),
  );
  m.connect(account());
  await m.load();
  slow = true;
  const reading = m.load(false, true);
  await m.command({ kind: 'create' });
  delayed.resolve({ epoch: '1', value: value() });
  await reading;
  assert.equal(m.view.value?.room?.id, 'new');
  m.dispose();
});
test('expired together session clears account state and invokes the auth handler', async () => {
  let expired = 0;
  const m = useTogether(
    client({
      togetherView: async () => {
        throw { code: 'session_expired', message: 'expired' };
      },
    }),
    () => {
      expired++;
    },
  );
  m.connect(account());
  await m.load(true);
  assert.equal(m.epoch.value, '');
  assert.equal(m.view.value, undefined);
  assert.equal(expired, 1);
  m.dispose();
});
test('close failure survives page refresh and releases both late and current account listeners', async () => {
  const registration = held<() => void>();
  const callbacks = new Map<string, (message: string) => void>();
  let stoppedOld = 0,
    stoppedCurrent = 0,
    shown = 0;
  const m = useTogether(
    client({
      onTogetherCloseError: async (epoch, callback) => {
        callbacks.set(epoch, callback);
        if (epoch === '1') return registration.promise;
        return () => {
          stoppedCurrent++;
        };
      },
    }),
    undefined,
    () => {
      shown++;
      m.show(true);
    },
  );
  m.connect(account());
  m.connect(account('2'));
  registration.resolve(() => {
    stoppedOld++;
  });
  await m.load(false);
  await Promise.resolve();
  callbacks.get('1')?.('old account');
  assert.equal(m.closeError.value, '');
  callbacks.get('2')?.('close requires reconciliation');
  await m.load(true);
  assert.equal(m.closeError.value, 'close requires reconciliation');
  assert.equal(shown, 1);
  await m.command({ kind: 'reconcile', operation_id: 'end-op' });
  assert.equal(m.closeError.value, '');
  m.dispose();
  callbacks.get('2')?.('disposed');
  assert.equal(m.closeError.value, '');
  assert.equal(stoppedOld, 1);
  assert.equal(stoppedCurrent, 1);
});
