import assert from 'node:assert/strict';
import test from 'node:test';
import { useColdOffline } from '../src/composables/useColdOffline.ts';
import type { ColdOfflineClient, ColdOfflinePage } from '../src/services/coldOffline.ts';

const song = { id: '1', title: 'Synthetic song', artist: 'Test', actualQuality: 'standard' };
const first = { items: [song], hasMore: true };
const idle = { phase: 'idle', playingId: null } as const;
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}
const turn = () => new Promise<void>((resolve) => setImmediate(resolve));
function client(overrides: Partial<ColdOfflineClient> = {}): ColdOfflineClient {
  return {
    coldOfflineOpen: async () => first,
    coldOfflineNext: async () => ({ items: [], hasMore: false }),
    coldOfflinePlay: async (id) => ({ phase: 'playing', playingId: id }),
    coldOfflineStop: async () => idle,
    coldOfflineStatus: async () => idle,
    coldOfflineClose: async () => {},
    ...overrides,
  };
}

test('cold offline only opens explicitly and clears a late list before closing', async () => {
  const response = deferred<ColdOfflinePage>();
  let opens = 0;
  let closes = 0;
  const model = useColdOffline(
    client({
      coldOfflineOpen: () => {
        opens++;
        return response.promise;
      },
      coldOfflineClose: async () => {
        closes++;
      },
    }),
  );
  await model.open();
  assert.equal(opens, 0);
  model.setEligible(true);
  assert.equal(opens, 0);
  const pending = model.open();
  await turn();
  assert.equal(opens, 1);
  model.setEligible(false);
  assert.equal(model.page.value, undefined);
  assert.equal(closes, 0);
  response.resolve(first);
  await pending;
  await turn();
  assert.equal(model.page.value, undefined);
  assert.equal(closes, 1);
});

test('empty eligible pages can advance and the playback state comes from the host', async () => {
  const model = useColdOffline(
    client({
      coldOfflineOpen: async () => ({ items: [], hasMore: true }),
      coldOfflineNext: async () => ({ items: [song], hasMore: false }),
    }),
  );
  model.setEligible(true);
  try {
    await model.open();
    assert.deepEqual(model.page.value?.items, []);
    await model.next();
    await model.play(song.id);
    assert.equal(model.currentTitle.value, song.title);
    await model.stop();
    assert.deepEqual(model.playback.value, idle);
  } finally {
    model.setEligible(false);
  }
});

test('a lost page reply closes the host cursor instead of silently advancing again', async () => {
  let reads = 0;
  let closes = 0;
  const model = useColdOffline(
    client({
      coldOfflineNext: async () => {
        reads++;
        throw Error('synthetic lost reply');
      },
      coldOfflineClose: async () => {
        closes++;
      },
    }),
  );
  model.setEligible(true);
  await model.open();
  await model.next();
  await turn();
  assert.equal(model.page.value, undefined);
  assert.equal(closes, 1);
  await model.next();
  assert.equal(reads, 1);
});

test('a revoked grant removes private rows and never repeats playback', async () => {
  const revoked = deferred<void>();
  let plays = 0;
  const model = useColdOffline(
    client({
      coldOfflinePlay: async () => {
        plays++;
        throw Error('lost playback reply');
      },
      coldOfflineStatus: async () => {
        throw Error('grant revoked');
      },
      coldOfflineClose: async () => revoked.resolve(),
    }),
  );
  model.setEligible(true);
  await model.open();
  await model.play(song.id);
  await revoked.promise;
  assert.equal(model.page.value, undefined);
  assert.equal(model.playback.value, undefined);
  assert.equal(plays, 1);
});

test('exit sends native Close while Play is pending and discards its late response', async () => {
  const playback = deferred<{ phase: 'playing'; playingId: string }>();
  let closes = 0;
  const model = useColdOffline(
    client({
      coldOfflinePlay: () => playback.promise,
      coldOfflineClose: async () => {
        closes++;
      },
    }),
  );
  model.setEligible(true);
  await model.open();
  const pending = model.play(song.id);
  await turn();
  model.leave();
  await turn();
  assert.equal(closes, 1);
  assert.equal(model.page.value, undefined);
  playback.resolve({ phase: 'playing', playingId: song.id });
  await pending;
  assert.equal(model.playback.value, undefined);
});
