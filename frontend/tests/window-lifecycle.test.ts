import { test } from 'node:test';
import assert from 'node:assert/strict';
import { useWindowLifecycle } from '../src/composables/useWindowLifecycle.ts';
import type { WindowLifecycleClient, WindowLifecycleView } from '../src/services/window.ts';
const view = (revision = '0'): WindowLifecycleView => ({
  revision,
  installed: true,
  trayAvailable: true,
  busy: false,
  hidden: false,
  message: null,
  prompt: null,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
test('native events overtake read and command replies without reopening an old close prompt', async () => {
  const initial = deferred<WindowLifecycleView>(),
    command = deferred<WindowLifecycleView>();
  let callback!: (view: WindowLifecycleView) => void;
  const client: WindowLifecycleClient = {
    windowLifecycle: () => initial.promise,
    onWindowLifecycle: async (fn) => {
      callback = fn;
      return () => {};
    },
    windowCloseCommand: () => command.promise,
  };
  const model = useWindowLifecycle(client);
  const start = model.connect();
  await Promise.resolve();
  callback({ ...view('9007199254740993'), hidden: true });
  initial.resolve(view('9007199254740992'));
  await start;
  assert.equal(model.view.value?.hidden, true);
  const pending = model.command({ kind: 'cancel', id: '1' });
  callback({ ...view('9007199254740995'), trayAvailable: false, message: 'restored' });
  command.resolve(view('9007199254740994'));
  await pending;
  assert.equal(model.view.value?.trayAvailable, false);
  assert.equal(model.view.value?.message, 'restored');
  model.dispose();
  callback(view('9007199254740996'));
  assert.equal(model.view.value?.revision, '9007199254740995');
});
test('a lost close acknowledgement only reads back and never repeats exit or hide', async () => {
  let calls = 0,
    reads = 0;
  const model = useWindowLifecycle({
    onWindowLifecycle: async () => () => {},
    windowLifecycle: async () => {
      reads++;
      return { ...view(String(reads)), hidden: reads > 1 };
    },
    windowCloseCommand: async () => {
      calls++;
      throw { message: 'lost reply' };
    },
  });
  await model.connect();
  await model.command({ kind: 'confirm', id: '1', behavior: 'tray', remember: true });
  assert.equal(calls, 1);
  assert.equal(reads, 2);
  assert.equal(model.view.value?.hidden, true);
  assert.ok(model.error.value);
  model.dispose();
});
test('listener registration completing after unmount is removed without an initial read', async () => {
  const subscribed = deferred<() => void>();
  let removed = 0,
    reads = 0;
  const model = useWindowLifecycle({
    onWindowLifecycle: () => subscribed.promise,
    windowLifecycle: async () => {
      reads++;
      return view();
    },
    windowCloseCommand: async () => view(),
  });
  const connecting = model.connect();
  model.dispose();
  subscribed.resolve(() => {
    removed++;
  });
  await connecting;
  assert.equal(removed, 1);
  assert.equal(reads, 0);
});
