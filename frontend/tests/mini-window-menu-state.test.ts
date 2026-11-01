import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, ref, shallowRef } from 'vue';
import { useMiniWindowMenu } from '../src/composables/useMiniWindowMenu.ts';
import type { MiniView, MiniLyricMenuView, MiniLyricMenuAction } from '../src/services/mini.ts';
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const view = shallowRef<MiniView | null>({
    generation: '1',
    ticket: 'm1',
    pinApplied: true,
    preferences: { revision: 'p', alwaysOnTop: true, damaged: false },
    media: {
      epoch: '1',
      selection: { audioPlayId: null, audioEntryId: null, videoPlayId: null },
      item: null,
      state: 'stopped',
      playing: false,
      canPrevious: false,
      canNext: false,
      position: null,
      duration: null,
      seekStart: 0,
      seekEnd: null,
      canSeek: false,
      volume: 100,
      mode: null,
      modeFamily: null,
      like: null,
      error: null,
    },
  });
  const busy = ref(false),
    calls: unknown[][] = [];
  const context: MiniLyricMenuView = {
    ticket: 'lyric-1',
    enabled: false,
    visible: false,
    opening: false,
    locked: false,
    damaged: false,
    error: null,
  };
  let reading: Promise<MiniLyricMenuView> | undefined, writing: Promise<void> | undefined;
  const scope = effectScope();
  const model = scope.run(() =>
    useMiniWindowMenu(
      view,
      {
        lyricMenu: async (ticket: string) => {
          calls.push(['read', ticket]);
          return reading ?? context;
        },
        lyricMenuAction: async (ticket: string, action: MiniLyricMenuAction) => {
          calls.push(['action', ticket, action]);
          await writing;
        },
      },
      busy,
      async () => {
        calls.push(['refresh']);
        return view.value ?? undefined;
      },
    ),
  )!;
  return {
    view,
    busy,
    scope,
    model,
    context,
    calls,
    read: (p: Promise<MiniLyricMenuView>) => {
      reading = p;
    },
    write: (p: Promise<void>) => {
      writing = p;
    },
  };
}
test('closing or changing account rejects delayed menu context without enabling a write', async () => {
  const f = fixture(),
    held = deferred<MiniLyricMenuView>();
  f.read(held.promise);
  const load = f.model.read();
  f.model.closed();
  held.resolve(f.context);
  await load;
  assert.equal(f.model.lyrics.value, undefined);
  await f.model.action('toggle');
  assert.equal(f.calls.length, 1);
  const later = deferred<MiniLyricMenuView>();
  f.read(later.promise);
  const again = f.model.read();
  f.view.value = { ...f.view.value!, media: { ...f.view.value!.media, epoch: '2' } };
  await nextTick();
  later.resolve(f.context);
  await again;
  assert.equal(f.model.lyrics.value, undefined);
  assert.equal(f.model.readIssue.value, '');
  f.scope.stop();
});
test('a consumed context and a lost write are never automatically submitted again', async () => {
  const f = fixture(),
    held = deferred<void>();
  await f.model.read();
  f.model.closed();
  f.write(held.promise);
  const action = f.model.action('toggle');
  await f.model.action('toggle');
  assert.equal(f.model.working.value, true);
  assert.equal(f.busy.value, false);
  assert.equal(f.model.lyrics.value, undefined);
  held.reject({ code: 'state', message: 'window apply failed' });
  await action;
  assert.equal(f.busy.value, false);
  assert.equal(f.model.issue.value, 'window apply failed');
  await f.model.action('toggle');
  assert.deepEqual(f.calls, [['read', 'm1'], ['action', 'lyric-1', 'toggle'], ['refresh']]);
  await f.model.read();
  assert.equal(f.model.issue.value, '');
  assert.equal(f.model.lyrics.value?.ticket, 'lyric-1');
  f.scope.stop();
});
test('old account operation results do not replace the next account menu state', async () => {
  const f = fixture(),
    held = deferred<void>();
  await f.model.read();
  f.model.closed();
  f.write(held.promise);
  const action = f.model.action('lock');
  f.view.value = { ...f.view.value!, media: { ...f.view.value!.media, epoch: '2' } };
  await nextTick();
  held.reject({ code: 'state', message: 'old failure' });
  await action;
  assert.equal(f.model.issue.value, '');
  assert.equal(f.model.lyrics.value, undefined);
  assert.equal(f.busy.value, false);
  f.scope.stop();
});
test('an open menu resumes observing external lyric changes after transport is busy', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const f = fixture();
  try {
    await f.model.read();
    f.busy.value = true;
    t.mock.timers.tick(750);
    await nextTick();
    assert.equal(f.calls.length, 1);
    assert.equal(f.model.lyrics.value?.visible, false);
    f.busy.value = false;
    f.read(Promise.resolve({ ...f.context, ticket: 'lyric-2', enabled: true, visible: true }));
    t.mock.timers.tick(750);
    await new Promise<void>((resolve) => setImmediate(resolve));
    assert.equal(f.calls.length, 2);
    assert.equal(f.model.lyrics.value?.visible, true);
    f.model.closed();
    t.mock.timers.tick(1500);
    await nextTick();
    assert.equal(f.calls.length, 2);
  } finally {
    f.scope.stop();
    t.mock.timers.reset();
  }
});
