import assert from 'node:assert/strict';
import test, { type TestContext } from 'node:test';
import { createRenderer, defineComponent, h, nextTick, ref, shallowRef } from 'vue';
import { useCollectionViewport } from '../src/composables/useCollectionViewport.ts';
import { useDownloadViewport } from '../src/composables/useDownloadViewport.ts';
import { useRankingViewport } from '../src/composables/useRankingViewport.ts';
import type { useRankings } from '../src/composables/useRankings.ts';
import type { DownloadModel } from '../src/composables/useDownloads.ts';
import { downloadViewportKey } from '../src/services/downloadViewport.ts';
import type { DownloadQuery } from '../src/services/downloads.ts';

function setup(t: TestContext) {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const frames = new Map<number, FrameRequestCallback>();
  let frameId = 0;
  const body = {},
    opener = {},
    other = {};
  const doc = { body, activeElement: opener };
  const replacements = {
    document: doc,
    requestAnimationFrame: (run: FrameRequestCallback) => {
      frames.set(++frameId, run);
      return frameId;
    },
    cancelAnimationFrame: (id: number) => frames.delete(id),
  };
  for (const [key, value] of Object.entries(replacements)) {
    const original = Object.getOwnPropertyDescriptor(globalThis, key);
    Object.defineProperty(globalThis, key, { configurable: true, value });
    t.after(() => {
      if (original) Object.defineProperty(globalThis, key, original);
      else Reflect.deleteProperty(globalThis, key);
    });
  }
  const host = () => ({});
  const renderer = createRenderer({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: host,
    createText: host,
    createComment: host,
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  function mount<T>(create: () => T) {
    let result!: T;
    const app = renderer.createApp(
      defineComponent({
        setup() {
          result = create();
          return () => h('div');
        },
      }),
    );
    app.mount({});
    t.after(() => app.unmount());
    return result;
  }
  function focusTarget(dataset: Record<string, string>, top = 40, bottom = 90) {
    const target = {
      dataset,
      isConnected: true,
      getBoundingClientRect: () => ({ top, bottom, height: bottom - top }),
      getClientRects: () => [{ top, bottom }],
      matches: () => false,
      focus: () => (doc.activeElement = target),
    };
    return target;
  }
  function emitFrame() {
    const current = [...frames.values()];
    frames.clear();
    current.forEach((run) => run(0));
  }
  return { doc, other, frames, mount, focusTarget, emitFrame };
}

async function settle() {
  // Flush Vue post watchers and the short promise chain after each layout barrier.
  for (let i = 0; i < 6; i++) await nextTick();
}

function downloadFixture(t: TestContext) {
  const ui = setup(t);
  const control = ui.focusTarget({ downloadAction: 'more' });
  const row = {
    ...ui.focusTarget({ downloadTask: 'target', downloadIndex: '6' }),
    querySelectorAll: () => [control],
  };
  const scroll = Object.assign(new EventTarget(), ui.focusTarget({}, 0, 500), {
    clientHeight: 500,
    dataset: { downloadColumns: '2', downloadPitch: '60' },
    scrollTop: 0,
    querySelectorAll: () => [row],
  });
  const root = shallowRef({ querySelector: () => scroll } as unknown as HTMLElement);
  const query = ref({
    tab: 'completed',
    category: 'song',
    keyword: '',
    sort: 'time',
    descending: true,
    offset: 0,
  } as DownloadQuery);
  const epoch = 'owner',
    visit = ref(1),
    growth = ref([0]);
  const view = shallowRef();
  const saved = {
    epoch,
    query: downloadViewportKey(query.value),
    anchor: 'target',
    index: 1,
    offset: 8,
    focus: { task: 'target', action: 'more' },
    growth: [2],
  };
  ui.mount(() =>
    useDownloadViewport(
      root,
      {
        viewport: ref(saved),
        visit,
        view,
        query,
        loading: ref(false),
        saveViewport() {},
      } as unknown as DownloadModel,
      growth,
    ),
  );
  view.value = {
    epoch,
    tasks: [...Array.from({ length: 6 }, (_, i) => ({ id: String(i) })), { id: 'target' }],
  };
  return { ...ui, control, scroll, growth, visit };
}

test('download viewport restores anchor and action after ordinary compositor frames', async (t) => {
  const f = downloadFixture(t);
  await settle();
  f.emitFrame();
  await settle();
  assert.equal(f.scroll.scrollTop, 0);
  f.emitFrame();
  await settle();
  assert.equal(f.scroll.scrollTop, 188);
  f.emitFrame();
  f.emitFrame();
  await settle();
  assert.equal(f.doc.activeElement, f.control);
  assert.deepEqual(f.growth.value, [2]);
  assert.equal(f.frames.size, 0);
});

test('download viewport settles when frames stall and does not steal changed focus or visit', async (t) => {
  const f = downloadFixture(t);
  await settle();
  assert.equal(f.frames.size, 1);
  t.mock.timers.tick(65);
  await settle();
  assert.equal(f.scroll.scrollTop, 188);
  f.doc.activeElement = f.other;
  f.visit.value++;
  t.mock.timers.tick(65);
  await settle();
  assert.equal(f.doc.activeElement, f.other);
  assert.equal(f.frames.size, 0);
});

test('collection restore survives missing frames and an unfinished finite animation', async (t) => {
  const f = setup(t);
  const card = f.focusTarget({ collectionKey: 'subscribed' });
  const scroll = Object.assign(new EventTarget(), { scrollTop: 0 });
  let animationRead = 0;
  const never = new Promise<void>(() => {});
  const element = {
    isConnected: true,
    contains: (node: object) => node === card,
    querySelector: () => scroll,
    querySelectorAll: () => [card],
    closest: () => ({
      getAnimations: () => {
        animationRead++;
        return [
          {
            playState: 'running',
            effect: { getComputedTiming: () => ({ endTime: 180 }) },
            finished: never,
          },
        ];
      },
    }),
  };
  const root = shallowRef(element as unknown as HTMLElement);
  const viewport = f.mount(() =>
    useCollectionViewport(
      root,
      () => true,
      () => true,
    ),
  );
  viewport.enter({ scrollTop: 240, focus: 'subscribed' });
  await settle();
  t.mock.timers.tick(65);
  await settle();
  assert.equal(animationRead, 1);
  assert.equal(scroll.scrollTop, 0);
  t.mock.timers.tick(451);
  await settle();
  assert.equal(scroll.scrollTop, 240);
  t.mock.timers.tick(65);
  await settle();
  assert.equal(f.doc.activeElement, card);
  assert.equal(f.frames.size, 0);

  // A pending old visit may resolve its timers but cannot scroll a later visit.
  viewport.enter({ scrollTop: 100, focus: 'subscribed' });
  await settle();
  viewport.cancel();
  t.mock.timers.tick(600);
  await settle();
  assert.equal(scroll.scrollTop, 240);
  assert.equal(f.frames.size, 0);
});

function rankingFixture(t: TestContext) {
  const f = setup(t);
  const control = f.focusTarget({ rankAction: 'play' });
  const card = {
    dataset: { rankId: 'chart' },
    querySelectorAll: () => [control],
  };
  const scroll = Object.assign(new EventTarget(), { scrollTop: 0 });
  let finish!: () => void;
  const finished = new Promise<void>((resolve) => (finish = resolve));
  const root = shallowRef({
    isConnected: true,
    querySelectorAll: () => [card],
    querySelector: () => undefined,
    closest: (selector: string) =>
      selector === '.main-content'
        ? scroll
        : {
            getAnimations: (options: GetAnimationsOptions) => {
              assert.equal(options.subtree, false);
              return [
                {
                  playState: 'running',
                  effect: { getComputedTiming: () => ({ endTime: 180 }) },
                  finished,
                },
              ];
            },
          },
  } as unknown as HTMLElement);
  let owner = true;
  f.mount(() =>
    useRankingViewport(
      {
        captureVisit: () => () => owner,
        restoration: ref({ scrollTop: 180, focus: { kind: 'card', id: 'chart', action: 'play' } }),
        view: ref({}),
        error: ref(''),
      } as unknown as ReturnType<typeof useRankings>,
      root,
    ),
  );
  return { ...f, control, scroll, finish, leave: () => (owner = false) };
}

test('ranking viewport restores the saved action after a normal frame and animation', async (t) => {
  const f = rankingFixture(t);
  await settle();
  f.emitFrame();
  await settle();
  assert.equal(f.scroll.scrollTop, 0);
  f.finish();
  await settle();
  assert.equal(f.scroll.scrollTop, 180);
  assert.equal(f.doc.activeElement, f.control);
  assert.equal(f.frames.size, 0);
});

test('ranking viewport restores when neither the frame nor the animation completes', async (t) => {
  const f = rankingFixture(t);
  await settle();
  t.mock.timers.tick(65);
  await settle();
  assert.equal(f.scroll.scrollTop, 0);
  t.mock.timers.tick(451);
  await settle();
  assert.equal(f.scroll.scrollTop, 180);
  assert.equal(f.doc.activeElement, f.control);
  assert.equal(f.frames.size, 0);
});

test('ranking deadline does not override deliberate scroll, focus or a departed visit', async (t) => {
  for (const departed of [false, true]) {
    await t.test(departed ? 'departed visit' : 'deliberate input', async (sub) => {
      const f = rankingFixture(sub);
      await settle();
      sub.mock.timers.tick(65);
      await settle();
      f.doc.activeElement = f.other;
      if (departed) f.leave();
      else f.scroll.dispatchEvent(new Event('wheel'));
      f.scroll.scrollTop = 25;
      sub.mock.timers.tick(451);
      await settle();
      assert.equal(f.scroll.scrollTop, 25);
      assert.equal(f.doc.activeElement, f.other);
      assert.equal(f.frames.size, 0);
    });
  }
});
