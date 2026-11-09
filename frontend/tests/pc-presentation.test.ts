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
  searchFooter: revision !== '1',
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
  push(appearance('2'));
  held.resolve(appearance('1'));
  await starting;
  assert.equal(memory.presentation('1')?.revision, '2');
  await host.start();
  assert.deepEqual(calls, ['listen', 'snapshot'], 'no polling or second shell subscription');
  host.dispose();
  push(appearance('3'));
  assert.equal(memory.presentation('1')?.revision, '2');
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
  second.resolve(appearance('0', '2'));
  await tick();
  push(appearance('99'));
  first.resolve(appearance('98'));
  await starting;
  assert.equal(memory.presentation('2')?.revision, '0');
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
        return appearance('1');
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

test('a late old event cannot roll back a newer completed snapshot or conflict with its flags', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  let push!: (view: PcPresentationView) => void;
  const host = usePcPresentation(
    {
      onPcPresentationChanged: async (listener) => {
        push = listener;
        return () => {};
      },
      pcPresentation: async () => appearance('2'),
    },
    memory,
  );
  host.connect(account);
  await host.start();
  assert.equal(memory.presentation('1')?.revision, '2');
  push(appearance('1'));
  push({ ...appearance('2'), searchFooter: false });
  assert.equal(memory.presentation('1')?.revision, '2');
  assert.equal(memory.presentation('1')?.searchFooter, true);
  host.dispose();
});

test('old, invalid and conflicting duplicate events do not block a genuinely fresh pending snapshot', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  memory.observePresentation(appearance('2'));
  const held = deferred<PcPresentationView>();
  let push!: (view: PcPresentationView) => void;
  const host = usePcPresentation(
    {
      onPcPresentationChanged: async (listener) => {
        push = listener;
        return () => {};
      },
      pcPresentation: () => held.promise,
    },
    memory,
  );
  host.connect(account);
  const starting = host.start();
  await tick();
  push(appearance('1'));
  push(appearance('18446744073709551616'));
  push(appearance('invalid'));
  push({ ...appearance('2'), searchFooter: false });
  held.resolve(appearance('3'));
  await starting;
  assert.equal(memory.presentation('1')?.revision, '3');
  assert.equal(memory.presentation('1')?.searchFooter, true);
  host.dispose();
});

test('an accepted intermediate event also cannot discard a newer snapshot by arrival serial', async () => {
  const memory = createSearchViewportMemory();
  memory.connect(account);
  const held = deferred<PcPresentationView>();
  let push!: (view: PcPresentationView) => void;
  const host = usePcPresentation(
    {
      onPcPresentationChanged: async (listener) => {
        push = listener;
        return () => {};
      },
      pcPresentation: () => held.promise,
    },
    memory,
  );
  host.connect(account);
  const starting = host.start();
  await tick();
  push(appearance('2'));
  held.resolve({ ...appearance('3'), searchDense: true });
  await starting;
  assert.equal(memory.presentation('1')?.revision, '3');
  assert.equal(memory.presentation('1')?.searchDense, true);
  host.dispose();
});

test('guest readiness is native, ordered within its own epoch, and never enables signed presentation', async () => {
  const memory = createSearchViewportMemory();
  const guest: AccountView = { ...account, status: 'unknown' };
  memory.connect(guest);
  const held = deferred<PcPresentationView>();
  let push!: (view: PcPresentationView) => void;
  const host = usePcPresentation(
    {
      onPcPresentationChanged: async (listener) => {
        push = listener;
        return () => {};
      },
      pcPresentation: (epoch) =>
        epoch === '1' ? held.promise : Promise.resolve(appearance('5', epoch)),
    },
    memory,
  );
  host.connect(guest);
  const starting = host.start();
  await tick();
  assert.equal(host.guestSearchReady.value, false);
  push({ ...appearance('2'), guestSearchReady: true });
  assert.equal(host.guestSearchReady.value, true);
  assert.equal(memory.presentation('1'), undefined);
  push({ ...appearance('3'), guestSearchReady: false });
  held.resolve({ ...appearance('1'), guestSearchReady: true });
  await starting;
  assert.equal(host.guestSearchReady.value, false);
  push({ ...appearance('4'), guestSearchReady: true });
  assert.equal(host.guestSearchReady.value, true);
  const signed = { ...account, epoch: '2' };
  memory.connect(signed);
  host.connect(signed);
  push({ ...appearance('6'), guestSearchReady: true });
  await tick();
  assert.equal(host.guestSearchReady.value, false);
  assert.equal(memory.presentation('2')?.revision, '5');
  host.dispose();
});

test('current podcast resume presentation follows ordered signed events and clears across accounts', async () => {
  let push!: (view: PcPresentationView) => void;
  const host = usePcPresentation(
    {
      onPcPresentationChanged: async (listener) => {
        push = listener;
        return () => {};
      },
      pcPresentation: async (epoch) => appearance('1', epoch),
    },
    createSearchViewportMemory(),
  );
  host.connect(account);
  await host.start();
  push({ ...appearance('3'), podcastResume: true });
  push(appearance('2'));
  assert.equal(host.podcastResume.value, true);
  push(appearance('4'));
  assert.equal(host.podcastResume.value, false);
  push({ ...appearance('5'), podcastResume: true });
  assert.equal(host.podcastResume.value, true);
  host.connect({ ...account, epoch: '2', status: 'signed_out' });
  push({ ...appearance('8'), podcastResume: true });
  push({ ...appearance('9', '2'), podcastResume: true });
  assert.equal(host.podcastResume.value, false);
  host.dispose();
});
