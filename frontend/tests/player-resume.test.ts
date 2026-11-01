import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createPlayerResume } from '../src/services/playerResume.ts';
const turn = () => new Promise((r) => setImmediate(r));

test('resume does not initialize a missing player or read while hidden', async () => {
  let calls = 0,
    visible = false;
  const ready = { value: false },
    resume = createPlayerResume(
      {
        ready,
        syncPlayback: async () => {
          calls++;
        },
      },
      () => visible,
    );
  await resume.wake();
  ready.value = true;
  await resume.wake();
  assert.equal(calls, 0);
  visible = true;
  await resume.wake();
  assert.equal(calls, 1);
  resume.dispose();
});
test('focus and visibility repairs are serialized and read once more after a delayed older snapshot', async () => {
  let calls = 0,
    release!: () => void;
  const held = new Promise<void>((r) => (release = r)),
    resume = createPlayerResume(
      {
        ready: { value: true },
        syncPlayback: async () => {
          calls++;
          if (calls === 1) await held;
        },
      },
      () => true,
    );
  const first = resume.wake();
  resume.wake();
  resume.wake();
  assert.equal(calls, 1);
  release();
  await first;
  assert.equal(calls, 2);
  resume.dispose();
});
test('disposed visibility repair cannot perform a queued read after returning to a new surface', async () => {
  let calls = 0,
    release!: () => void;
  const held = new Promise<void>((r) => (release = r)),
    resume = createPlayerResume(
      {
        ready: { value: true },
        syncPlayback: async () => {
          calls++;
          await held;
        },
      },
      () => true,
    );
  const first = resume.wake();
  resume.wake();
  resume.dispose();
  release();
  await first;
  await resume.wake();
  assert.equal(calls, 1);
});
test('snapshot repair failure does not replay anything, and a later explicit focus can read again', async () => {
  let calls = 0;
  const resume = createPlayerResume(
    {
      ready: { value: true },
      syncPlayback: async () => {
        calls++;
        throw Error('Synthetic read failure');
      },
    },
    () => true,
  );
  await resume.wake();
  await turn();
  assert.equal(calls, 1);
  await resume.wake();
  assert.equal(calls, 2);
  resume.dispose();
});
