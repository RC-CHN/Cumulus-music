import { test } from 'node:test';
import assert from 'node:assert/strict';
import { prepareMiniViewport, type MiniViewport } from '../src/services/miniViewport.ts';

test('an allocated mini viewport completes a single native handover', async () => {
  const calls: MiniViewport[] = [];
  await prepareMiniViewport(
    async (v) => {
      calls.push(v);
      return true;
    },
    () => ({ width: 420, height: 206, scale: 2 }),
    async () => {
      assert.fail('no layout wait needed');
    },
  );
  assert.deepEqual(calls, [{ width: 420, height: 206, scale: 2 }]);
});
test('fractional WebView scale is negotiated before accepting the corrected viewport', async () => {
  let current = { width: 336, height: 164, scale: 1.25 };
  const calls: MiniViewport[] = [];
  await prepareMiniViewport(
    async (v) => {
      calls.push({ ...v });
      return v.width === 420 && v.height === 206;
    },
    () => current,
    async () => {
      current = { width: 420, height: 206, scale: 1.25 };
    },
  );
  assert.deepEqual(calls, [
    { width: 336, height: 164, scale: 1.25 },
    { width: 420, height: 206, scale: 1.25 },
  ]);
});
test('unallocated geometry has a fixed attempt and wait limit', async () => {
  let calls = 0,
    waits = 0;
  await assert.rejects(
    prepareMiniViewport(
      async () => {
        calls++;
        return false;
      },
      () => ({ width: 0, height: 0, scale: 1 }),
      async () => {
        waits++;
      },
    ),
    /迷你窗口尺寸尚未就绪/,
  );
  assert.equal(calls, 3);
  assert.equal(waits, 60);
});
test('a revoked native lifetime is never retried as another window', async () => {
  let calls = 0;
  const stale = { code: 'stale', message: 'window changed' };
  await assert.rejects(
    prepareMiniViewport(
      async () => {
        calls++;
        throw stale;
      },
      () => ({ width: 420, height: 206, scale: 1 }),
      async () => {
        assert.fail('stale lifetime must stop');
      },
    ),
    (e) => e === stale,
  );
  assert.equal(calls, 1);
});
