import assert from 'node:assert/strict';
import test from 'node:test';
import { usePcPresentation } from '../src/composables/usePcPresentation.ts';
import { createSearchViewportMemory } from '../src/services/searchViewport.ts';
import type { AccountView } from '../src/services/contracts.ts';
import type { PcPresentationView } from '../src/services/pcPresentation.ts';

const account: AccountView = {
  epoch: '1',
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
};
const appearance = (revision: string, epoch = '1'): PcPresentationView => ({
  epoch,
  revision,
  searchFooter: revision !== 'off',
  searchExpanded: true,
  searchDense: false,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
const tick = () => new Promise<void>((done) => setImmediate(done));

test('the shell subscribes before snapshot and a newer event beats its late snapshot', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  const held = deferred<PcPresentationView>();
  let push!: (view: PcPresentationView) => void;
  const calls: string[] = [];
  let stopped = 0;
  const host = usePcPresentation(
    {
      onPcPresentationChanged: async (listener) => {
        calls.push('listen');
        push = listener;
        return () => {
          stopped++;
        };
      },
      pcPresentation: () => {
        calls.push('snapshot');
        return held.promise;
      },
    },
    memory,
  );
  host.connect(account);
  assert.deepEqual(calls, []);
  const starting = host.start();
  await tick();
  assert.deepEqual(calls, ['listen', 'snapshot']);
  push(appearance('new'));
  held.resolve(appearance('off'));
  await starting;
  assert.equal(memory.presentation('1')?.revision, 'new');
  await host.start();
  assert.deepEqual(calls, ['listen', 'snapshot'], 'no polling or second shell subscription');
  host.dispose();
  push(appearance('after-dispose'));
  assert.equal(memory.presentation('1')?.revision, 'new');
  assert.equal(stopped, 1);
});

test('account switches discard old events and snapshots without touching the new owner', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  const first = deferred<PcPresentationView>(),
    second = deferred<PcPresentationView>();
  let push!: (view: PcPresentationView) => void;
  const calls: string[] = [];
  const host = usePcPresentation(
    {
      onPcPresentationChanged: async (listener) => {
        push = listener;
        return () => {};
      },
      pcPresentation: (epoch) => {
        calls.push(epoch);
        return epoch === '1' ? first.promise : second.promise;
      },
    },
    memory,
  );
  host.connect(account);
  const starting = host.start();
  await tick();
  const other = { ...account, epoch: '2' };
  memory.connect(other);
  host.connect(other);
  second.resolve(appearance('second', '2'));
  await tick();
  push(appearance('old event'));
  first.resolve(appearance('old snapshot'));
  await starting;
  assert.equal(memory.presentation('2')?.revision, 'second');
  assert.equal(memory.presentation('1'), undefined);
  assert.deepEqual(calls, ['1', '2']);
  host.dispose();
});

test('disposal while listener registration is pending releases it without a snapshot', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  const listener = deferred<() => void>();
  let snapshots = 0,
    stopped = 0;
  const host = usePcPresentation(
    {
      onPcPresentationChanged: () => listener.promise,
      pcPresentation: async () => {
        snapshots++;
        return appearance('unexpected');
      },
    },
    memory,
  );
  host.connect(account);
  const starting = host.start();
  host.dispose();
  listener.resolve(() => {
    stopped++;
  });
  await starting;
  assert.equal(stopped, 1);
  assert.equal(snapshots, 0);
});
