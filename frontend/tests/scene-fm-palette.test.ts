import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useSceneFmPalette } from '../src/composables/useSceneFmPalette.ts';
import {
  SCENE_FM_THEME_FALLBACK,
  sceneFmBackgroundCss,
  type SceneFmBackground,
  type SceneFmPaletteClient,
  type SceneFmPaletteRead,
} from '../src/services/sceneFmPalette.ts';
import type { AccountData } from '../src/services/contracts.ts';

const gradient: SceneFmBackground = {
  kind: 'gradient',
  first: { r: 179, g: 72, b: 72 },
  middle: { r: 179, g: 72, b: 89 },
};
const other: SceneFmBackground = {
  kind: 'gradient',
  first: { r: 72, g: 179, b: 72 },
  middle: { r: 89, g: 179, b: 72 },
};
const scope = (voiceId = '601', epoch = '1') => ({
  epoch,
  voiceId,
  revision: '2',
  coverKey: `https://p1.music.126.net/fixture-${voiceId}.png`,
});
const sample = (receipt = 'opaque'): SceneFmPaletteRead => ({
  kind: 'sample',
  receipt,
  mimeType: 'image/png',
  bytes: [1, 2, 3],
  width: 1,
  height: 1,
});
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const reads: unknown[][] = [],
    samples: unknown[][] = [],
    cancelled: string[] = [];
  const core: SceneFmPaletteClient = {
    async sceneFmPaletteRead(...args) {
      reads.push(args);
      return { epoch: args[0], value: { kind: 'ready', background: gradient } };
    },
    async sceneFmPaletteSample(...args) {
      samples.push(args);
      return { epoch: args[0], value: { background: other } };
    },
    async sceneFmPaletteCancel(epoch, receipt) {
      cancelled.push(`${epoch}:${receipt}`);
    },
  };
  return { core, reads, samples, cancelled };
}

test('numeric native background uses exact gradient stops, theme fallback and safe NaN serialization', () => {
  assert.equal(
    sceneFmBackgroundCss(gradient),
    'linear-gradient(245deg, rgb(179, 72, 72) 0%, rgb(179, 72, 89) 48%, rgb(179, 72, 72) 100%)',
  );
  assert.equal(sceneFmBackgroundCss({ kind: 'theme_fallback' }), SCENE_FM_THEME_FALLBACK);
  assert.ok(
    sceneFmBackgroundCss({ ...gradient, first: { r: null, g: 0, b: 0 } }).includes(
      'rgb(NaN, 0, 0)',
    ),
  );
  assert.ok(
    !sceneFmBackgroundCss({
      ...gradient,
      first: { r: 'url(evil)' as unknown as number, g: 0, b: 0 },
    }).includes('evil'),
  );
});

test('pause-only view revisions retain the current source palette without another read', async () => {
  const f = fixture(),
    model = useSceneFmPalette(f.core);
  await model.show(scope(), 1.5);
  assert.deepEqual(f.reads, [['1', '2', '601', 1.5]]);
  assert.deepEqual(model.background.value, gradient);
  await model.show({ ...scope(), revision: '99' }, 2);
  assert.equal(f.reads.length, 1);
  assert.deepEqual(model.background.value, gradient);
  model.dispose();
});

test('settled same-account same-cover colors and fallback survive a voice change without another read', async () => {
  for (const kind of ['header', 'sample', 'fallback'] as const) {
    const f = fixture();
    const expected: SceneFmBackground =
      kind === 'fallback' ? { kind: 'theme_fallback' } : kind === 'sample' ? other : gradient;
    f.core.sceneFmPaletteRead = async (...args) => {
      f.reads.push(args);
      return {
        epoch: args[0],
        value: kind === 'sample' ? sample() : { kind: 'ready', background: expected },
      };
    };
    const model = useSceneFmPalette(f.core, async () => ({
      width: 1,
      height: 1,
      rgba: [11, 13, 17, 255],
    }));
    const first = scope();
    await model.show(first, 1);
    await model.show({ ...scope('602'), coverKey: first.coverKey, revision: '3' }, 1);
    assert.deepEqual(model.background.value, expected, kind);
    assert.equal(model.pending.value, false);
    assert.equal(f.reads.length, 1, kind);
    assert.equal(f.samples.length, kind === 'sample' ? 1 : 0, kind);
    await model.show({ ...scope('602', '2'), coverKey: first.coverKey }, 1);
    assert.equal(f.reads.length, 2, 'another account never inherits the settled image');
    model.dispose();
  }
});

test('ordinary native/image failures settle the official default for later voices with the same cover', async () => {
  for (const kind of ['network', 'bounded_media', 'local_decode'] as const) {
    const f = fixture();
    f.core.sceneFmPaletteRead = async (...args) => {
      f.reads.push(args);
      if (kind === 'local_decode') return { epoch: args[0], value: sample() };
      throw {
        code: kind === 'network' ? 'network' : 'operation',
        message: 'Synthetic image acquisition failure',
        retryable: kind === 'network',
      };
    };
    const model = useSceneFmPalette(f.core, async () => {
      throw Error('Synthetic local decode failure');
    });
    const first = scope();
    await model.show(first, 1);
    await model.show({ ...scope('602'), coverKey: first.coverKey }, 1);
    assert.equal(model.css.value, SCENE_FM_THEME_FALLBACK, kind);
    assert.equal(f.reads.length, 1, kind);
    assert.deepEqual(f.samples, []);
    assert.deepEqual(f.cancelled, kind === 'local_decode' ? ['1:opaque'] : []);
    model.dispose();
  }
});

test('pending or stale old-voice work cannot count as a settled same-cover palette', async () => {
  for (const code of ['stale', 'profile_changed', 'session_expired']) {
    for (const staleBeforeSwitch of [false, true]) {
      const f = fixture(),
        late = deferred<AccountData<SceneFmPaletteRead>>();
      f.core.sceneFmPaletteRead = (...args) => {
        f.reads.push(args);
        return args[2] === '601'
          ? late.promise
          : Promise.resolve({ epoch: args[0], value: { kind: 'ready', background: other } });
      };
      const model = useSceneFmPalette(f.core);
      const first = scope();
      const old = model.show(first, 1);
      const reject = () =>
        late.reject({ code, message: 'Synthetic native voice changed', retryable: false });
      if (staleBeforeSwitch) {
        reject();
        await old;
      }
      await model.show({ ...scope('602'), coverKey: first.coverKey }, 1);
      if (!staleBeforeSwitch) {
        reject();
        await old;
      }
      assert.deepEqual(
        f.reads.map((args) => args[2]),
        ['601', '602'],
      );
      assert.deepEqual(model.background.value, other);
      assert.equal(model.pending.value, false);
      model.dispose();
    }
  }
});

test('same-account cover changes keep old color until the replacement finishes, then use native fallback on failure', async () => {
  const f = fixture(),
    model = useSceneFmPalette(f.core);
  await model.show(scope(), 1);
  const late = deferred<AccountData<SceneFmPaletteRead>>();
  f.core.sceneFmPaletteRead = () => late.promise;
  const next = model.show(scope('602'), 1);
  assert.equal(model.pending.value, true);
  assert.deepEqual(model.background.value, gradient);
  late.reject(Error('decode/transport unavailable'));
  await next;
  assert.equal(model.pending.value, false);
  assert.equal(model.css.value, SCENE_FM_THEME_FALLBACK);
  model.dispose();
});

test('an older source response cannot recolor the current voice and its late receipt is cancelled', async () => {
  const f = fixture(),
    late = deferred<AccountData<SceneFmPaletteRead>>();
  f.core.sceneFmPaletteRead = (epoch, _, voice) =>
    voice === '601'
      ? late.promise
      : Promise.resolve({ epoch, value: { kind: 'ready', background: other } });
  const model = useSceneFmPalette(f.core);
  const old = model.show(scope(), 1);
  await model.show(scope('602'), 1);
  late.resolve({ epoch: '1', value: sample('late') });
  await old;
  assert.deepEqual(model.background.value, other);
  assert.deepEqual(f.cancelled, ['1:late']);
  model.dispose();
});

test('sample fallback only decodes native bytes, then submits receipt-bound full RGBA', async () => {
  const f = fixture();
  f.core.sceneFmPaletteRead = async (epoch) => ({ epoch, value: sample() });
  const model = useSceneFmPalette(f.core, async (image, signal) => {
    assert.equal(image.kind, 'sample');
    assert.equal(signal.aborted, false);
    assert.deepEqual(image.bytes, [1, 2, 3]);
    return { width: 1, height: 1, rgba: [11, 13, 17, 0] };
  });
  await model.show(scope(), 1);
  assert.deepEqual(f.samples, [['1', 'opaque', 1, 1, [11, 13, 17, 0]]]);
  assert.deepEqual(model.background.value, other);
  assert.deepEqual(f.cancelled, []);
  model.dispose();
});

test('header-only fallback does not request browser decode or sample', async () => {
  const f = fixture();
  f.core.sceneFmPaletteRead = async (epoch) => ({
    epoch,
    value: { kind: 'ready', background: { kind: 'theme_fallback' } },
  });
  const model = useSceneFmPalette(f.core, async () => {
    throw Error('unexpected browser decode');
  });
  await model.show(scope(), 1);
  assert.equal(model.css.value, SCENE_FM_THEME_FALLBACK);
  assert.deepEqual(f.samples, []);
  model.dispose();
});

test('account switch clears old color immediately and rejects an old completion', async () => {
  const f = fixture(),
    late = deferred<AccountData<SceneFmPaletteRead>>();
  const model = useSceneFmPalette(f.core);
  await model.show(scope(), 1);
  f.core.sceneFmPaletteRead = () => late.promise;
  const next = model.show(scope('602', '2'), 1);
  assert.equal(model.css.value, SCENE_FM_THEME_FALLBACK);
  late.resolve({ epoch: '1', value: { kind: 'ready', background: gradient } });
  await next;
  assert.equal(model.css.value, SCENE_FM_THEME_FALLBACK);
  model.dispose();
});

test('unmount cancels an active receipt and stops a late canvas from reaching native quantization', async () => {
  const f = fixture(),
    pixels = deferred<{ width: number; height: number; rgba: number[] }>();
  let signal: AbortSignal | undefined;
  f.core.sceneFmPaletteRead = async (epoch) => ({ epoch, value: sample() });
  const model = useSceneFmPalette(f.core, async (_, captured) => {
    signal = captured;
    return pixels.promise;
  });
  const old = model.show(scope(), 1);
  await Promise.resolve();
  assert.equal(signal?.aborted, false);
  model.dispose();
  assert.equal(signal?.aborted, true);
  pixels.resolve({ width: 1, height: 1, rgba: [0, 0, 0, 255] });
  await old;
  assert.deepEqual(f.samples, []);
  assert.deepEqual(f.cancelled, ['1:opaque']);
  assert.equal(model.pending.value, false);
});

test('a late quantized result cannot overwrite a new cover; absent same-account covers retain color', async () => {
  const f = fixture(),
    late = deferred<AccountData<{ background: SceneFmBackground }>>();
  f.core.sceneFmPaletteRead = async (epoch, _, voice) => ({
    epoch,
    value: voice === '601' ? sample() : { kind: 'ready', background: other },
  });
  f.core.sceneFmPaletteSample = () => late.promise;
  const model = useSceneFmPalette(f.core, async () => ({
    width: 1,
    height: 1,
    rgba: [0, 0, 0, 255],
  }));
  const old = model.show(scope(), 1);
  await Promise.resolve();
  await Promise.resolve();
  await model.show(scope('602'), 1);
  late.resolve({ epoch: '1', value: { background: gradient } });
  await old;
  assert.deepEqual(model.background.value, other);
  await model.show({ ...scope('603'), coverKey: null }, 1);
  assert.deepEqual(model.background.value, other);
  assert.deepEqual(f.cancelled, ['1:opaque']);
  model.dispose();
});

test('a same-source view that first lacked revision can begin once authoritative scope arrives', async () => {
  const f = fixture(),
    model = useSceneFmPalette(f.core);
  await model.show({ ...scope(), revision: null }, 1);
  assert.equal(f.reads.length, 0);
  await model.show(scope(), 1);
  assert.deepEqual(f.reads, [['1', '2', '601', 1]]);
  assert.deepEqual(model.background.value, gradient);
  model.dispose();
});
