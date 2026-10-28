import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useRelayPublication } from '../src/composables/useRelayPublication.ts';
import {
  publicationOutcome,
  type RelayPublicationClient,
  type RelayPublicationView,
} from '../src/services/relayPublication.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: 'Synthetic',
  avatarUrl: null,
};
const blocked = () => ({
  id: 'blocked',
  phase: 'uncertain' as const,
  recoveryOf: null,
  serviceCode: null,
  createdAt: 1,
});
const page = (): RelayPublicationView => ({
  pending: 0,
  blocked: blocked(),
  recovering: null,
  latest: blocked(),
  requested: null,
  canResynchronize: true,
  sending: false,
  recordFailed: false,
  preflightFailed: false,
});
const reply = (value = page(), epoch = '1') => ({ value: structuredClone(value), epoch });
const backend = (extra: Partial<RelayPublicationClient> = {}): RelayPublicationClient => ({
  relayPublicationView: async (e) => reply(page(), e),
  relayPublicationCommand: async (e) => reply(page(), e),
  ...extra,
});
test('publication requires the current blocked receipt and suppresses repeated clicks', async () => {
  let finish!: (v: ReturnType<typeof reply>) => void;
  const held = new Promise<ReturnType<typeof reply>>((r) => (finish = r));
  let calls = 0;
  const model = useRelayPublication(
    backend({
      relayPublicationCommand: async () => {
        calls++;
        return held;
      },
    }),
  );
  model.connect(account);
  await model.read();
  assert.equal(await model.resynchronize('stale'), false);
  assert.equal(calls, 0);
  const work = model.resynchronize('blocked');
  assert.equal(await model.resynchronize('blocked'), false);
  assert.equal(calls, 1);
  finish(reply({ ...page(), blocked: null, canResynchronize: false }));
  await work;
  assert.equal(model.busy.value, false);
  model.dispose();
});
test('lost recovery acknowledgement reads the exact durable receipt without another write', async () => {
  const value = page();
  let writes = 0,
    requested = '';
  const model = useRelayPublication(
    backend({
      relayPublicationView: async (_, id) => {
        requested = id ?? '';
        return reply(value);
      },
      relayPublicationCommand: async (_, command) => {
        writes++;
        value.blocked = null;
        value.canResynchronize = false;
        value.requested = {
          id: command.id,
          phase: 'confirmed',
          recoveryOf: 'blocked',
          createdAt: 2,
          serviceCode: null,
        };
        throw { code: 'network', message: 'lost' };
      },
    }),
  );
  model.connect(account);
  await model.read();
  await model.resynchronize('blocked');
  assert.equal(writes, 1);
  assert.equal(requested, value.requested?.id);
  assert.equal(model.unknown.value, undefined);
  assert.equal(model.error.value, '');
  assert.equal(model.page.value?.requested?.phase, 'confirmed');
  model.dispose();
});
test('unseen recovery blocks new work and uses a local cancellation fence', async () => {
  const value = page();
  const calls: string[] = [];
  const model = useRelayPublication(
    backend({
      relayPublicationView: async () => reply(value),
      relayPublicationCommand: async (_, cmd) => {
        calls.push(cmd.kind);
        if (cmd.kind === 'resynchronize') throw { code: 'network', message: 'lost' };
        value.requested = {
          id: cmd.id,
          phase: 'cancelled',
          recoveryOf: cmd.blocked_id,
          createdAt: 2,
          serviceCode: null,
        };
        return reply(value);
      },
    }),
  );
  model.connect(account);
  await model.read();
  await model.resynchronize('blocked');
  assert.ok(model.unknown.value);
  await model.resynchronize('blocked');
  assert.deepEqual(calls, ['resynchronize']);
  await model.resolveUnknown();
  assert.deepEqual(calls, ['resynchronize', 'cancel_unseen']);
  assert.equal(model.unknown.value, undefined);
  assert.equal(model.page.value?.requested?.phase, 'cancelled');
  model.dispose();
});
test('prepared retry reuses its UUID and refuses sent or unknown work', async () => {
  const value = page();
  value.recovering = {
    id: 'prepared-id',
    phase: 'prepared',
    recoveryOf: 'blocked',
    createdAt: 2,
    serviceCode: null,
  };
  value.canResynchronize = false;
  const ids: string[] = [];
  const model = useRelayPublication(
    backend({
      relayPublicationView: async () => reply(value),
      relayPublicationCommand: async (_, cmd) => {
        ids.push(cmd.id);
        return reply(value);
      },
    }),
  );
  model.connect(account);
  await model.read();
  await model.resumePrepared();
  assert.deepEqual(ids, ['prepared-id']);
  value.recovering.phase = 'sending';
  await model.read();
  assert.equal(await model.resumePrepared(), false);
  assert.equal(ids.length, 1);
  model.dispose();
});
test('account changes discard late status and recovery responses', async () => {
  let finish!: (v: ReturnType<typeof reply>) => void;
  const held = new Promise<ReturnType<typeof reply>>((r) => (finish = r));
  const model = useRelayPublication(backend({ relayPublicationCommand: async () => held }));
  model.connect(account);
  await model.read();
  const work = model.resynchronize('blocked');
  model.connect({ ...account, epoch: '2' });
  finish(reply());
  await work;
  assert.equal(model.page.value, undefined);
  assert.equal(model.busy.value, false);
  assert.equal(model.unknown.value, undefined);
  model.dispose();
});
test('recording failure is distinct from audio and read failure preserves an explicit error', async () => {
  const value = page();
  value.recordFailed = true;
  value.canResynchronize = false;
  assert.match(publicationOutcome(value), /本机播放不受影响/);
  let offline = false;
  const model = useRelayPublication(
    backend({
      relayPublicationView: async () => {
        if (offline) throw { code: 'network', message: 'read unavailable' };
        return reply(value);
      },
    }),
  );
  model.connect(account);
  await model.read();
  assert.equal(model.canResynchronize.value, false);
  offline = true;
  await model.read();
  assert.equal(model.error.value, 'read unavailable');
  assert.equal(model.page.value?.recordFailed, true);
  model.dispose();
});

test('background reads leave recovery controls usable and cannot replace a newer outcome', async () => {
  let hold = false;
  let finish!: (value: ReturnType<typeof reply>) => void;
  const held = new Promise<ReturnType<typeof reply>>((resolve) => (finish = resolve));
  const model = useRelayPublication(
    backend({
      relayPublicationView: async () => (hold ? held : reply()),
      relayPublicationCommand: async () =>
        reply({ ...page(), blocked: null, canResynchronize: false }),
    }),
  );
  model.connect(account);
  await model.read();
  hold = true;
  const background = model.read(true);
  assert.equal(model.loading.value, false);
  assert.equal(model.canResynchronize.value, true);
  await model.resynchronize('blocked');
  finish(reply());
  await background;
  assert.equal(model.page.value?.blocked, null);
  model.dispose();
});
