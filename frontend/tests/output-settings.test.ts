import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useOutputSettings } from '../src/composables/useOutputSettings.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type { OutputClient, OutputView } from '../src/services/output.ts';
const account = (epoch = '1') => ({
  epoch,
  status: 'signed_out' as const,
  nickname: '',
  avatarUrl: null,
});
const value = (): OutputView => ({
  revision: '0',
  selected: { id: 'auto', description: '系统默认' },
  damaged: false,
  supported: true,
  devices: [{ id: 'pulse/a', description: '耳机' }],
  engines: [],
});
const reply = (view = value(), epoch = '1') => ({ value: structuredClone(view), epoch });
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
function backend(extra: Partial<OutputClient> = {}): OutputClient {
  return {
    outputSettings: async (e) => reply(value(), e),
    outputCommand: async (e) => reply(value(), e),
    ...extra,
  };
}
test('output change waits for persistence and repeated writes do not pass the busy guard', async () => {
  const gate = deferred<AccountData<OutputView>>();
  let calls = 0;
  const model = useOutputSettings(
    backend({
      outputCommand: async (_, command) => {
        calls++;
        assert.equal(command.kind, 'select');
        return gate.promise;
      },
    }),
  );
  try {
    await model.connect(account());
    const pending = model.select('pulse/a');
    assert.equal(model.view.value?.selected.id, 'auto');
    assert.equal(await model.retry(), false);
    gate.resolve(
      reply({ ...value(), revision: '1', selected: { id: 'pulse/a', description: '耳机' } }),
    );
    assert.equal(await pending, true);
    assert.equal(model.saved.value, true);
    assert.equal(calls, 1);
  } finally {
    model.dispose();
  }
});
test('lost output write acknowledgement reads current state without reissuing selection', async () => {
  let stored = value(),
    writes = 0;
  const model = useOutputSettings(
    backend({
      outputSettings: async () => reply(stored),
      outputCommand: async () => {
        writes++;
        stored = { ...value(), revision: '1', selected: { id: 'pulse/a', description: '耳机' } };
        throw { code: 'network', message: 'lost reply' };
      },
    }),
  );
  try {
    await model.connect(account());
    assert.equal(await model.select('pulse/a'), false);
    assert.equal(model.view.value?.selected.id, 'pulse/a');
    assert.equal(model.saved.value, false);
    assert.equal(await model.retry(), false);
    assert.equal(writes, 1);
    await model.read();
    assert.equal(model.canChange.value, true);
  } finally {
    model.dispose();
  }
});
test('new account and disposed settings ignore late reads and mutations', async () => {
  const gate = deferred<AccountData<OutputView>>();
  const model = useOutputSettings(
    backend({
      outputSettings: (e) => (e === '1' ? gate.promise : Promise.resolve(reply(value(), e))),
    }),
  );
  try {
    const pending = model.connect(account());
    await model.connect(account('2'));
    gate.resolve(reply({ ...value(), damaged: true }));
    await pending;
    assert.equal(model.view.value?.damaged, false);
  } finally {
    model.dispose();
  }
  const write = deferred<AccountData<OutputView>>();
  const other = useOutputSettings(backend({ outputCommand: () => write.promise }));
  await other.connect(account());
  const pending = other.select('pulse/a');
  other.dispose();
  write.resolve(reply({ ...value(), revision: '1' }));
  assert.equal(await pending, false);
  assert.equal(other.view.value?.revision, '0');
});
test('unsupported and damaged settings never masquerade as available output selection', async () => {
  const unsupported = useOutputSettings(
    backend({ outputSettings: async () => reply({ ...value(), supported: false }) }),
  );
  try {
    await unsupported.connect(account());
    assert.equal(unsupported.canChange.value, false);
    assert.equal(await unsupported.select('pulse/a'), false);
  } finally {
    unsupported.dispose();
  }
  const damaged = useOutputSettings(
    backend({
      outputSettings: async () => reply({ ...value(), damaged: true, revision: null }),
      outputCommand: async (_, command) => {
        assert.equal(command.kind, 'repair');
        return reply();
      },
    }),
  );
  try {
    await damaged.connect(account());
    assert.equal(damaged.canChange.value, false);
    assert.equal(await damaged.repair(), true);
    assert.equal(damaged.canChange.value, true);
  } finally {
    damaged.dispose();
  }
});
