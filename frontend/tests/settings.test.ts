import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useSettings } from '../src/composables/useSettings.ts';
import type {
  SettingsClient,
  AccountSettingsPage,
  AccountSettingsCommand,
} from '../src/services/settings.ts';
const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: '合成账号',
  avatarUrl: null,
};
function page(): AccountSettingsPage {
  return {
    revision: 'a'.repeat(64),
    fetchedAt: 1,
    fields: [{ key: 'shareSetting', value: 0, state: 'available' }],
    linkedPrivacy: {
      peopleCanSeePlayRecord: true,
      followedCanSeePlayRecord: true,
      socialPlaylistSetting: 0,
    },
    operations: [],
  };
}
const reply = (value = page(), epoch = '1') => ({ epoch, value: structuredClone(value) });
const backend = (overrides: Partial<SettingsClient> = {}): SettingsClient => ({
  accountSettingsPage: async (e) => reply(page(), e),
  accountSettingsCommand: async (e) => reply(page(), e),
  ...overrides,
});
const intent = (): AccountSettingsCommand => ({
  kind: 'change',
  id: '00000000-0000-0000-0000-000000000001',
  expected_revision: 'a'.repeat(64),
  change: { key: 'shareSetting', value: 1 },
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
test('settings preserves cached values offline and blocks editing until a successful refresh', async () => {
  let offline = false;
  const model = useSettings(
    backend({
      accountSettingsPage: async (e, refresh) => {
        if (refresh && offline) throw { code: 'network', message: '离线' };
        return reply(page(), e);
      },
    }),
  );
  model.connect(account);
  await model.open();
  assert.equal(model.canChange.value, true);
  offline = true;
  await model.refresh();
  assert.equal(model.page.value?.fields[0].value, 0);
  assert.equal(model.error.value, '离线');
  assert.equal(model.canChange.value, false);
  offline = false;
  await model.refresh();
  assert.equal(model.canChange.value, true);
});
test('settings never optimistically replaces an option and reconciles lost replies from native journal', async () => {
  const gate = deferred<ReturnType<typeof reply>>();
  let reads = 0,
    writes = 0;
  const value = page();
  const model = useSettings(
    backend({
      accountSettingsPage: async () => {
        reads++;
        return reply(value);
      },
      accountSettingsCommand: async () => {
        writes++;
        await gate.promise;
        value.operations = [
          {
            id: '00000000-0000-0000-0000-000000000001',
            change: { key: 'shareSetting', value: 1 },
            before: 0,
            observed: null,
            phase: 'uncertain',
            createdAt: 1,
            lastReadAt: null,
            readRevision: null,
          },
        ];
        throw { code: 'network', message: '响应丢失' };
      },
    }),
  );
  model.connect(account);
  await model.open();
  const pending = model.command(intent());
  assert.equal(model.page.value?.fields[0].value, 0);
  assert.equal(model.canChange.value, false);
  assert.equal(await model.command(intent()), false);
  gate.resolve(reply());
  await pending;
  assert.equal(model.unknown.value, undefined);
  assert.equal(model.pending.value, true);
  assert.equal(writes, 1);
  assert.equal(reads, 3);
});
test('unseen IPC intent is retained across reads and cancelled with its original identity', async () => {
  const calls: AccountSettingsCommand[] = [];
  const model = useSettings(
    backend({
      accountSettingsCommand: async (e, c) => {
        calls.push(c);
        if (c.kind === 'change') throw { code: 'network', message: 'IPC丢失' };
        const p = page();
        if (c.kind === 'cancel_unseen')
          p.operations = [
            {
              id: c.id,
              change: c.change,
              before: null,
              observed: null,
              phase: 'cancelled',
              createdAt: 1,
              lastReadAt: null,
              readRevision: null,
            },
          ];
        return reply(p, e);
      },
    }),
  );
  model.connect(account);
  await model.open();
  await model.command(intent());
  assert.equal(model.pending.value, true);
  await model.localRefresh();
  assert.equal(model.pending.value, true);
  await model.cancelUnknown();
  assert.equal(model.pending.value, false);
  assert.deepEqual(calls, [intent(), { ...intent(), kind: 'cancel_unseen' }]);
});
test('known preflight conflict displays the refreshed snapshot without inventing an unresolved write', async () => {
  const p = page();
  const model = useSettings(
    backend({
      accountSettingsPage: async () => reply(p),
      accountSettingsCommand: async () => {
        p.fields[0].value = 1;
        p.revision = 'b'.repeat(64);
        throw { code: 'settings_changed', message: '设置已变化' };
      },
    }),
  );
  model.connect(account);
  await model.open();
  await model.command(intent());
  assert.equal(model.page.value?.fields[0].value, 1);
  assert.equal(model.unknown.value, undefined);
  assert.equal(model.canChange.value, false);
});
test('account switch and dispose reject late setting results and guest never calls settings API', async () => {
  for (const action of ['switch', 'dispose']) {
    const gate = deferred<ReturnType<typeof reply>>();
    let calls = 0;
    const model = useSettings(
      backend({
        accountSettingsPage: async (e, refresh) => {
          calls++;
          return refresh ? gate.promise : reply(page(), e);
        },
      }),
    );
    await model.open();
    assert.equal(calls, 0);
    model.connect(account);
    const work = model.open();
    await new Promise((r) => setImmediate(r));
    if (action === 'switch') model.connect({ ...account, epoch: '2' });
    else model.dispose();
    gate.resolve(reply());
    await work;
    assert.equal(model.page.value, undefined);
  }
});
test('session expiration clears settings and releases UI busy state even without bootstrap callback', async () => {
  let expired = 0;
  const model = useSettings(
    backend({
      accountSettingsCommand: async () => {
        throw { code: 'session_expired', message: '登录失效' };
      },
    }),
    () => expired++,
  );
  model.connect(account);
  await model.open();
  await model.command(intent());
  assert.equal(expired, 1);
  assert.equal(model.page.value, undefined);
  assert.equal(model.busy.value, false);
  assert.equal(model.loading.value, false);
  assert.equal(model.canChange.value, false);
});
