import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useRelay } from '../src/composables/useRelay.ts';
import {
  relayOutcome,
  relayPlaybackDisposition,
  type RelayClient,
  type RelayView,
} from '../src/services/relay.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'Synthetic',
  avatarUrl: null,
};
const page = (): RelayView => ({
  revision: 'a'.repeat(64),
  availability: 'available',
  setting: true,
  alert: { title: 'Continue', deviceName: 'Fixture', resourceName: 'Song', buttonText: 'Continue' },
  operations: [],
});
const reply = (value = page(), epoch = '1') => ({ value: structuredClone(value), epoch });
const backend = (extra: Partial<RelayClient> = {}): RelayClient => ({
  relayView: async (e) => reply(page(), e),
  relayCommand: async (e) => reply(page(), e),
  relaySettingCommand: async (e) => reply(page(), e),
  ...extra,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
test('relay distinguishes a confirmed unsent failure from a lost remote result', () => {
  const operation = {
    id: 'synthetic',
    phase: 'failed' as const,
    pullPhase: null,
    canRetry: false,
    createdAt: 1,
  };
  assert.match(relayOutcome(operation), /尚未发送/);
  assert.match(relayOutcome({ ...operation, pullPhase: 'uncertain' }), /未确认接续结果/);
  assert.match(relayOutcome({ ...operation, pullPhase: 'rejected' }), /被服务端拒绝/);
  assert.match(relayOutcome({ ...operation, phase: 'uncertain' }), /未确认接续结果/);
});
test('relay consumes only an explicit continuation and ignores duplicate clicks while pending', async () => {
  const gate = deferred<ReturnType<typeof reply>>();
  let calls = 0;
  const model = useRelay(
    backend({
      relayCommand: async () => {
        calls++;
        return gate.promise;
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  assert.equal(calls, 0);
  const operation = model.continueHere();
  assert.equal(model.alert.value, undefined);
  assert.equal(model.dismissalNotice.value, false);
  assert.equal(await model.continueHere(), false);
  assert.equal(calls, 1);
  gate.resolve(reply({ ...page(), alert: null }));
  await operation;
  assert.equal(model.busy.value, false);
});
test('only closing the current offer shows the settings notice for three seconds', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const gate = deferred<ReturnType<typeof reply>>();
  const dismissals: unknown[] = [];
  const model = useRelay(
    backend({
      relayCommand: async (epoch, command) => {
        dismissals.push({ epoch, command });
        return gate.promise;
      },
    }),
  );
  try {
    assert.equal(model.offer.value, undefined);
    model.connect(account);
    await model.read(true);
    const offer = model.offer.value!;
    await model.dismiss({ ...offer, revision: 'superseded' }, 'close');
    assert.equal(model.dismissalNotice.value, false);
    assert.deepEqual(dismissals, []);

    const close = model.dismiss(offer, 'close');
    assert.equal(model.offer.value, undefined);
    assert.equal(model.dismissalNotice.value, true);
    assert.deepEqual(dismissals, [
      { epoch: account.epoch, command: { kind: 'dismiss', revision: offer.revision } },
    ]);
    t.mock.timers.tick(2999);
    await model.dismiss(offer, 'close');
    assert.equal(model.dismissalNotice.value, true);
    t.mock.timers.tick(1);
    assert.equal(model.dismissalNotice.value, false);
    gate.resolve(reply({ ...page(), alert: null }));
    await close;
    assert.equal(model.dismissalNotice.value, false);
    assert.equal(dismissals.length, 1);
  } finally {
    model.dispose();
  }
});
test('relay timeout is silent and an old account cannot close an identical replacement offer', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const dismissals: string[] = [];
  const model = useRelay(
    backend({
      relayCommand: async (epoch, command) => {
        assert.equal(command.kind, 'dismiss');
        dismissals.push(epoch);
        return reply({ ...page(), alert: null }, epoch);
      },
    }),
  );
  try {
    model.connect(account);
    await model.read(true);
    const original = model.offer.value!;
    await model.dismiss(original, 'close');
    assert.equal(model.dismissalNotice.value, true);
    t.mock.timers.tick(1000);
    const next = { ...account, epoch: '2' };
    model.connect(next);
    assert.equal(model.dismissalNotice.value, false);
    await model.read(true);
    const replacement = model.offer.value!;
    assert.equal(replacement.revision, original.revision);
    await model.dismiss(original, 'close');
    assert.equal(model.dismissalNotice.value, false);
    assert.deepEqual(dismissals, [account.epoch]);
    assert.deepEqual(model.offer.value, replacement);

    await model.dismiss(replacement, 'timeout');
    assert.equal(model.dismissalNotice.value, false);
    assert.equal(model.offer.value, undefined);
    assert.deepEqual(dismissals, [account.epoch, next.epoch]);
  } finally {
    model.dispose();
  }
});
test('closing during refresh hides only that offer without interrupting the read', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const pending = deferred<ReturnType<typeof reply>>();
  let reads = 0;
  const model = useRelay(
    backend({
      relayView: async () => (++reads === 2 ? pending.promise : reply()),
      relayCommand: async () => assert.fail('dismiss must not interrupt the active read'),
    }),
  );
  try {
    model.connect(account);
    await model.read(true);
    const offer = model.offer.value!;
    const refresh = model.read(true);
    assert.equal(model.loading.value, true);
    await model.dismiss(offer, 'close');
    assert.equal(model.offer.value, undefined);
    assert.equal(model.dismissalNotice.value, true);
    assert.equal(model.loading.value, true);
    pending.resolve(reply());
    await refresh;
    assert.equal(model.offer.value, undefined);
    assert.equal(model.loading.value, false);
    assert.equal(reads, 2);
  } finally {
    model.dispose();
  }
});
test('relay lost IPC acknowledgement inspects native history without repeating the pull', async () => {
  let calls = 0,
    requestId = '';
  const value = page();
  const model = useRelay(
    backend({
      relayView: async () => reply(value),
      relayCommand: async (_, cmd) => {
        calls++;
        if ('id' in cmd) requestId = cmd.id;
        value.alert = null;
        value.operations = [
          { id: requestId, phase: 'installed', pullPhase: 'ready', canRetry: false, createdAt: 1 },
        ];
        throw { code: 'network', message: 'lost' };
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  await model.continueHere();
  assert.equal(calls, 1);
  assert.equal(model.unknown.value, undefined);
  assert.equal(model.page.value?.operations[0].phase, 'installed');
  assert.equal(model.playbackTarget.value, undefined);
  assert.equal(await model.continueHere(), false);
});
test('relay playback detail consumes only the explicit command and matching loaded player instance', async () => {
  const detail = { operationId: 'historic', playId: 'play-2', entryId: 'entry-2', revision: '12' };
  const model = useRelay(
    backend({
      relayView: async () => reply({ ...page(), loadedPlayback: detail }),
      relayCommand: async (_, command) => {
        assert.equal(command.kind, 'continue');
        if (command.kind !== 'continue') throw new Error('unexpected command');
        return reply({
          ...page(),
          loadedPlayback: { ...detail, operationId: command.id },
          operations: [
            {
              id: command.id,
              phase: 'installed',
              pullPhase: 'ready',
              canRetry: false,
              createdAt: 1,
            },
          ],
        });
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  assert.equal(model.playbackTarget.value, undefined);
  await model.continueHere();
  const target = model.playbackTarget.value;
  assert.ok(target);
  await model.read(false);
  assert.deepEqual(model.playbackTarget.value, target);
  const current = {
    epoch: account.epoch,
    playId: detail.playId,
    entryId: detail.entryId,
    revision: detail.revision,
    state: 'playing' as const,
  };
  assert.equal(relayPlaybackDisposition(target, { ...current, revision: '11' }), 'wait');
  assert.equal(relayPlaybackDisposition(target, current), 'open');
  assert.equal(relayPlaybackDisposition(target, { ...current, playId: 'replayed' }), 'discard');
  assert.equal(relayPlaybackDisposition(target, { ...current, entryId: 'replacement' }), 'discard');
  assert.equal(relayPlaybackDisposition(target, { ...current, state: 'idle' }), 'discard');
  assert.equal(relayPlaybackDisposition(target, { ...current, epoch: '2' }), 'discard');
  model.connect({ ...account, epoch: '2' });
  assert.equal(model.playbackTarget.value, undefined);
});
test('relay ignores a loaded receipt when the explicit operation failed or the account changed', async () => {
  for (const changed of [false, true]) {
    const gate = deferred<ReturnType<typeof reply>>();
    let operationId = '';
    const model = useRelay(
      backend({
        relayCommand: async (_, command) => {
          if (command.kind === 'continue') operationId = command.id;
          return gate.promise;
        },
      }),
    );
    model.connect(account);
    await model.read(true);
    const pending = model.continueHere();
    if (changed) model.connect({ ...account, epoch: '2' });
    gate.resolve(
      reply({
        ...page(),
        loadedPlayback: { operationId, playId: 'p', entryId: 'e', revision: '1' },
        operations: [
          {
            id: operationId,
            phase: changed ? 'installed' : 'failed',
            pullPhase: 'ready',
            canRetry: false,
            createdAt: 1,
          },
        ],
      }),
    );
    await pending;
    assert.equal(model.playbackTarget.value, undefined);
  }
});
test('relay unresolved host reply is cancelled before delayed IPC can execute', async () => {
  let calls = 0;
  const commands: string[] = [];
  const value = page();
  const model = useRelay(
    backend({
      relayView: async () => reply(value),
      relayCommand: async (_, cmd) => {
        calls++;
        commands.push(cmd.kind);
        if (cmd.kind === 'continue') throw { code: 'network', message: 'lost' };
        if (cmd.kind === 'cancel_unseen')
          value.operations = [
            {
              id: cmd.id,
              phase: 'cancelled',
              pullPhase: 'cancelled',
              canRetry: false,
              createdAt: 1,
            },
          ];
        return reply(value);
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  await model.continueHere();
  assert.ok(model.unknown.value);
  await model.continueHere();
  assert.equal(calls, 1);
  await model.cancelUnknown();
  assert.deepEqual(commands, ['continue', 'cancel_unseen']);
  assert.equal(model.unknown.value, undefined);
});
test('relay read failure retains display but blocks continue and account switch rejects late data', async () => {
  let offline = false;
  const gate = deferred<ReturnType<typeof reply>>();
  let held = false;
  const model = useRelay(
    backend({
      relayView: async () => {
        if (held) return gate.promise;
        if (offline) throw { code: 'network', message: 'offline' };
        return reply();
      },
    }),
  );
  model.connect(account);
  await model.read(true);
  offline = true;
  await model.read(true);
  assert.ok(model.page.value?.alert);
  assert.equal(await model.continueHere(), false);
  held = true;
  const work = model.read(true);
  model.connect({ ...account, epoch: '2' });
  gate.resolve(reply());
  await work;
  assert.equal(model.page.value, undefined);
  assert.equal(model.loading.value, false);
});
test('relay expiration releases pending flags and terminal plans cannot be retried', async () => {
  let expired = 0;
  const model = useRelay(
    backend({
      relayCommand: async () => {
        throw { code: 'session_expired', message: 'expired' };
      },
    }),
    () => expired++,
  );
  model.connect(account);
  await model.read(true);
  assert.equal(await model.retry('unlisted'), false);
  await model.continueHere();
  assert.equal(expired, 1);
  assert.equal(model.busy.value, false);
  assert.equal(model.page.value, undefined);
  assert.equal(model.unknown.value, undefined);
});
