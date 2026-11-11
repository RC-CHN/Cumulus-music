import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useTogether } from '../src/composables/useTogether.ts';
import {
  togetherRecordDuration,
  type TogetherClient,
  type TogetherLeaveRecord,
  type TogetherView,
} from '../src/services/together.ts';
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
    togetherStatistics: async (epoch) => ({ epoch, value: { listenCount: 0, totalSeconds: 0 } }),
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
  failed.room = { id: 'room', role: 'waiting', members: [], relationKey: null };
  failed.attached = true;
  failed.connection = { state: 'failed', message: '消息连接失败', received: 1, attempt: 4 };
  const m = useTogether(
    client({
      togetherStatistics: async (epoch) => ({ epoch, value: { listenCount: 0, totalSeconds: 0 } }),
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
  old.room = { id: 'private-old-room', role: 'waiting', members: [], relationKey: null };
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
      togetherStatistics: async (epoch) => ({ epoch, value: { listenCount: 0, totalSeconds: 0 } }),
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
        v.room = { id: 'new', role: 'waiting', members: [], relationKey: null };
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

test('relation statistics are read only for a visible pair and discard a late previous pair', async () => {
  const previous = held<AccountData<{ listenCount: number; totalSeconds: number }>>();
  const requests: string[] = [];
  const room = value();
  room.room = { id: 'room', role: 'together_owner', members: [], relationKey: 'first' };
  const model = useTogether(
    client({
      togetherView: async (epoch) => ({ epoch, value: { ...room, room: { ...room.room! } } }),
      togetherStatistics: async (epoch, relationKey) => {
        requests.push(relationKey);
        if (relationKey === 'first') return previous.promise;
        return { epoch, value: { listenCount: 12, totalSeconds: 3661 } };
      },
    }),
  );
  model.connect(account());
  await model.load();
  assert.deepEqual(requests, []);
  model.show(true);
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(requests, ['first']);
  room.room.relationKey = 'second';
  await model.load();
  assert.deepEqual(model.statistics.value, { listenCount: 12, totalSeconds: 3661 });
  previous.resolve({ epoch: '1', value: { listenCount: 99, totalSeconds: 3 } });
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(model.statistics.value, { listenCount: 12, totalSeconds: 3661 });
  model.show(false);
  assert.equal(model.statistics.value, undefined);
  model.dispose();
});

function record(operationId = 'verified-end'): TogetherLeaveRecord {
  return {
    operationId,
    thisListenCount: 3,
    thisDuration: 35,
    totalListenCount: 27,
    totalDuration: 3665,
    selfAvatarUrl: null,
    peerAvatarUrl: null,
  };
}

test('together record uses the official seconds-to-minutes rounding', () => {
  assert.deepEqual([0, 59, 60, 119, 3599, 3600, 3665].map(togetherRecordDuration), [
    '1分钟',
    '1分钟',
    '1分钟',
    '1分钟',
    '59分钟',
    '1小时0分钟',
    '1小时1分钟',
  ]);
});

test('together displays only a native confirmed numeric record with no unresolved room', async () => {
  const receipt = record();
  let current: TogetherView = {
    ...value(),
    room: { id: 'active', role: 'together', members: [], relationKey: null },
    leaveRecord: receipt,
  };
  const m = useTogether(client({ togetherView: async (epoch) => ({ epoch, value: current }) }));
  m.connect(account());
  await m.load();
  assert.equal(m.leaveRecord.value, undefined, 'active room never shows a record');
  current = {
    ...value(),
    pending: [{ id: 'uncertain', action: 'end', phase: 'uncertain', acknowledged: false }],
    leaveRecord: receipt,
  };
  await m.load();
  assert.equal(m.leaveRecord.value, undefined, 'unresolved exit never shows a record');
  current = { ...value(), leaveRecord: { ...receipt, thisDuration: -1 } };
  await m.load();
  assert.equal(m.leaveRecord.value, undefined);
  current = { ...value(), leaveRecord: { ...receipt, totalListenCount: 1.5 } };
  await m.load();
  assert.equal(m.leaveRecord.value, undefined);
  current = value();
  await m.load();
  assert.equal(m.leaveRecord.value, undefined, 'absence never manufactures counts');
  current = { ...value(), leaveRecord: receipt };
  await m.load();
  assert.deepEqual(
    m.leaveRecord.value,
    receipt,
    'runtime playlist clear may arrive via native view',
  );
  current = { ...value(), leaveRecord: { ...receipt, totalDuration: 1 } };
  await m.load();
  assert.deepEqual(m.leaveRecord.value, receipt, 'published receipt is an immutable presentation');
  m.dispose();
});

test('together record dismisses locally once and an older read cannot reopen it', async () => {
  const receipt = record();
  const older = held<AccountData<TogetherView>>();
  const dismissal = held<AccountData<TogetherView>>();
  const newRoom = { id: 'new', role: 'waiting' as const, members: [], relationKey: null };
  let heldRead = false;
  let created = false;
  let dismissals = 0;
  const m = useTogether(
    client({
      togetherView: async (epoch) =>
        heldRead ? older.promise : { epoch, value: { ...value(), room: created ? newRoom : null } },
      togetherCommand: async (epoch, command) => {
        if (command.kind === 'end') return { epoch, value: { ...value(), leaveRecord: receipt } };
        assert.deepEqual(command, {
          kind: 'dismiss_leave_record',
          operation_id: receipt.operationId,
        });
        dismissals++;
        return dismissal.promise;
      },
    }),
  );
  m.connect(account());
  await m.load();
  await m.command({ kind: 'end', room_id: 'confirmed' });
  assert.deepEqual(m.leaveRecord.value, receipt);
  heldRead = true;
  const read = m.load();
  const close = m.dismissLeaveRecord(receipt.operationId);
  assert.equal(m.leaveRecord.value, undefined);
  await m.dismissLeaveRecord(receipt.operationId);
  assert.equal(dismissals, 1);
  older.resolve({ epoch: '1', value: { ...value(), leaveRecord: receipt } });
  await read;
  assert.equal(
    m.leaveRecord.value,
    undefined,
    'already-pending poll is ignored for retired receipt',
  );
  heldRead = false;
  created = true;
  await m.load();
  assert.deepEqual(m.view.value?.room, newRoom);
  dismissal.resolve({ epoch: '1', value: { ...value(), leaveRecord: receipt } });
  await close;
  assert.equal(m.leaveRecord.value, undefined);
  assert.deepEqual(m.view.value?.room, newRoom, 'late dismiss cannot overwrite a new room');
  m.dispose();
});

test('new room and account switch discard ended room records and late end responses', async () => {
  const receipt = record();
  const late = held<AccountData<TogetherView>>();
  let current: TogetherView = { ...value(), leaveRecord: receipt };
  const m = useTogether(
    client({
      togetherView: async (epoch) => ({ epoch, value: epoch === '1' ? current : value() }),
      togetherCommand: () => late.promise,
    }),
  );
  m.connect(account());
  await m.load();
  assert.deepEqual(m.leaveRecord.value, receipt);
  current = {
    ...value(),
    room: { id: 'new', role: 'together_owner', members: [], relationKey: null },
  };
  await m.load();
  assert.equal(m.leaveRecord.value, undefined);
  current = { ...value(), leaveRecord: receipt };
  await m.load();
  assert.equal(m.leaveRecord.value, undefined, 'a retired receipt cannot survive room replacement');
  const end = m.command({ kind: 'end', room_id: 'new' });
  m.connect(account('2'));
  late.resolve({ epoch: '1', value: { ...value(), leaveRecord: record('late-end') } });
  await end;
  assert.equal(m.epoch.value, '2');
  assert.equal(m.leaveRecord.value, undefined);
  m.dispose();
});
