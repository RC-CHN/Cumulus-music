import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  sampleSceneCover,
  sceneSampleDimensions,
  type SceneCanvasEnvironment,
} from '../src/services/sceneFmCanvas.ts';

function fixture(
  width = 200,
  height = 100,
  options: { noContext?: boolean; incomplete?: boolean } = {},
) {
  const operations: unknown[] = [];
  const image = {
    naturalWidth: width,
    naturalHeight: height,
    width: 0,
    height: 0,
    onload: null as null | (() => void),
    onerror: null as null | (() => void),
    src: '',
    removeAttribute(name: string) {
      operations.push(['remove', name]);
    },
  };
  const context = {
    drawImage(...args: unknown[]) {
      operations.push(['draw', ...args.slice(1)]);
    },
    getImageData(x: number, y: number, w: number, h: number) {
      operations.push(['get', x, y, w, h]);
      const data = new Uint8ClampedArray(w * h * 4 - (options.incomplete ? 1 : 0));
      data.set([7, 11, 13, 0]);
      return { data };
    },
    set imageSmoothingEnabled(_: boolean) {
      throw Error('changed default smoothing');
    },
    set imageSmoothingQuality(_: string) {
      throw Error('changed default smoothing quality');
    },
  };
  const canvas = {
    width: 0,
    height: 0,
    getContext(kind: string) {
      operations.push(['context', kind]);
      return options.noContext ? null : context;
    },
  };
  const environment: SceneCanvasEnvironment = {
    image: () => image as unknown as HTMLImageElement,
    canvas: () => canvas as unknown as HTMLCanvasElement,
    objectUrl(bytes, mime) {
      operations.push(['blob', [...bytes], mime]);
      return 'blob:fixture';
    },
    revoke(url) {
      operations.push(['revoke', url]);
    },
  };
  return {
    image,
    canvas,
    environment,
    operations,
    source: { mimeType: 'image/png', bytes: [1, 2, 3], width, height },
  };
}

test('official canvas keeps narrow images and uses ceil after width reduction', () => {
  assert.deepEqual(sceneSampleDimensions(2, 3), { width: 2, height: 3 });
  assert.deepEqual(sceneSampleDimensions(101, 99), { width: 50, height: 50 });
  assert.deepEqual(sceneSampleDimensions(600, 100), { width: 50, height: 9 });
  assert.deepEqual(sceneSampleDimensions(50, 10), { width: 50, height: 10 });
  for (const [w, h] of [
    [0, 10],
    [1.2, 10],
    [4097, 2],
    [4096, 4096],
    [1, 4096],
  ]) {
    // 1x4096 is permitted; no unnecessary square resize or aspect rejection.
    if (w === 1 && h === 4096)
      assert.deepEqual(sceneSampleDimensions(w, h), { width: 1, height: 4096 });
    else assert.throws(() => sceneSampleDimensions(w, h));
  }
});

test('local image draws the full sampled canvas with browser defaults and preserves RGBA', async () => {
  const f = fixture();
  const pending = sampleSceneCover(f.source, new AbortController().signal, f.environment);
  assert.equal(f.image.src, 'blob:fixture');
  f.image.onload!();
  const value = await pending;
  assert.deepEqual({ width: value.width, height: value.height }, { width: 50, height: 25 });
  assert.deepEqual(value.rgba.slice(0, 4), [7, 11, 13, 0]);
  assert.deepEqual(f.operations, [
    ['blob', [1, 2, 3], 'image/png'],
    ['context', '2d'],
    ['draw', 0, 0, 50, 25],
    ['get', 0, 0, 50, 25],
    ['remove', 'src'],
    ['revoke', 'blob:fixture'],
  ]);
  assert.deepEqual([f.image.width, f.image.height], [200, 100]);
});

test('aborted decode revokes its local blob and cannot submit late pixels', async () => {
  const f = fixture();
  const abort = new AbortController();
  const pending = sampleSceneCover(f.source, abort.signal, f.environment);
  const lateLoad = f.image.onload!;
  abort.abort();
  await assert.rejects(pending, /cancelled/);
  lateLoad();
  assert.equal(f.operations.filter((entry) => (entry as string[])[0] === 'revoke').length, 1);
  assert.equal(f.operations.filter((entry) => (entry as string[])[0] === 'draw').length, 0);
});

test('invalid MIME, bytes and a pre-cancelled scope never create a blob', async () => {
  const f = fixture();
  for (const patch of [
    { mimeType: 'image/svg+xml' },
    { bytes: [] },
    { bytes: [256] },
    { bytes: [1.5] },
    { bytes: [-1] },
  ]) {
    await assert.rejects(
      sampleSceneCover({ ...f.source, ...patch }, new AbortController().signal, f.environment),
      /Invalid/,
    );
  }
  const abort = new AbortController();
  abort.abort();
  await assert.rejects(sampleSceneCover(f.source, abort.signal, f.environment), /cancelled/);
  assert.deepEqual(f.operations, []);
});

test('decoded dimensions must match the native receipt; failures always release blob URLs', async () => {
  for (const problem of ['dimensions', 'context', 'pixels', 'decode']) {
    const f = fixture(200, 100, {
      noContext: problem === 'context',
      incomplete: problem === 'pixels',
    });
    const source = problem === 'dimensions' ? { ...f.source, width: 199 } : f.source;
    const pending = sampleSceneCover(source, new AbortController().signal, f.environment);
    if (problem === 'decode') f.image.onerror!();
    else f.image.onload!();
    await assert.rejects(pending);
    assert.equal(f.operations.filter((entry) => (entry as string[])[0] === 'revoke').length, 1);
  }
});
