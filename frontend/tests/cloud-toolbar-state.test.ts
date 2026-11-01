import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, ref } from 'vue';
import type { CloudPage, CloudQuery } from '../src/services/cloud.ts';
import { readCloudCollection } from '../src/services/cloudCollection.ts';
import { useCloudToolbar, type CloudToolbarSource } from '../src/composables/useCloudToolbar.ts';
const query: CloudQuery = {
  tab: 'uploaded',
  keyword: '合成',
  sort: 'title',
  descending: true,
  offset: 200,
  limit: 200,
};
function page(offset: number, total = 405): CloudPage {
  return {
    epoch: '1',
    revision: 'r1',
    updatedMs: 1,
    size: 12,
    maxSize: 100,
    uploadedCount: total,
    uploadingCount: 0,
    total,
    offset,
    uploads: [],
    deletions: [],
    workerError: null,
    tracks: Array.from({ length: Math.min(200, total - offset) }, (_, i) => {
      const id = String(total - offset - i);
      return {
        track: {
          id,
          name: '合成',
          artists: [],
          album: '',
          coverUrl: null,
          durationMs: 1000,
          fee: 0,
          missing: false,
          local: false,
        },
        fileName: `${id}.flac`,
        fileSize: 12,
        addedAt: 1,
        bitrate: 1,
        format: 'flac',
      };
    }),
  };
}
function source(): CloudToolbarSource {
  return { epoch: '1', revision: 'r1', query: { ...query } };
}
function held<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
test('toolbar collection retains captured filter/order and reads every page instead of visible 200', async () => {
  const calls: [CloudQuery, boolean][] = [],
    input = { ...query };
  const result = await readCloudCollection(
    {
      cloudPage: async (_e, q, remote) => {
        calls.push([q, remote]);
        input.keyword = 'later';
        return page(q.offset);
      },
    },
    '1',
    input,
    () => true,
    () => {},
    { revision: 'r1', maximum: 5000, action: '下载' },
  );
  assert.equal(result.total, 405);
  assert.equal(result.tracks[0]!.track.id, '405');
  assert.equal(result.tracks.at(-1)!.track.id, '1');
  assert.deepEqual(
    calls.map(([q, r]) => [q.offset, r]),
    [
      [0, true],
      [200, false],
      [400, false],
    ],
  );
  assert(calls.every(([q]) => q.keyword === '合成' && q.sort === 'title' && q.descending));
});
test('displayed revision, malformed total, and operation limits reject before using a partial catalog', async () => {
  for (const mode of ['changed', 'over', 'fractional']) {
    let calls = 0;
    await assert.rejects(
      readCloudCollection(
        {
          cloudPage: async () => {
            calls++;
            const value = page(0);
            if (mode === 'changed') value.revision = 'r2';
            if (mode === 'over') value.total = 5001;
            if (mode === 'fractional') value.total = 2.5;
            return value;
          },
        },
        '1',
        query,
        () => true,
        () => {},
        { revision: 'r1', maximum: 5000, action: '下载' },
      ),
    );
    assert.equal(calls, 1);
  }
});
test('query replacement, account replacement, and disposal abandon pending catalog reads without invoking actions', async () => {
  for (const mode of ['query', 'account', 'dispose']) {
    const input = ref<CloudToolbarSource | undefined>(source()),
      delayed = held<CloudPage>();
    let calls = 0,
      actions = 0;
    const scope = effectScope();
    const state = scope.run(() =>
      useCloudToolbar(
        {
          cloudPage: async () => {
            calls++;
            return delayed.promise;
          },
        },
        () => input.value,
        async () => {
          actions++;
          return true;
        },
        () => {},
      ),
    )!;
    const pending = state.run('queue');
    if (mode === 'query') input.value = { ...source(), query: { ...query, keyword: 'new' } };
    else if (mode === 'account') input.value = { ...source(), epoch: '2' };
    else scope.stop();
    delayed.resolve(page(0));
    await pending;
    assert.equal(calls, 1);
    assert.equal(actions, 0);
    assert.equal(state.message.value, '');
    scope.stop();
  }
});
test('read failure requires an explicit new operation, and successful forwarding keeps all IDs/query/revision', async () => {
  let fail = true,
    reads = 0;
  const actions: unknown[] = [],
    scope = effectScope();
  const state = scope.run(() =>
    useCloudToolbar(
      {
        cloudPage: async (_e, q) => {
          reads++;
          if (fail) throw { code: 'io', message: '合成读取失败' };
          return page(q.offset);
        },
      },
      source,
      async (action, data, q) => {
        actions.push([action, data.revision, data.tracks.map((t) => t.track.id), q]);
        return true;
      },
      () => {},
    ),
  )!;
  await state.run('download');
  assert.equal(reads, 1);
  assert.equal(actions.length, 0);
  assert.equal(state.error.value, '合成读取失败');
  fail = false;
  await state.run('download');
  assert.equal(reads, 4);
  assert.equal(actions.length, 1);
  const sent = actions[0] as [string, string, string[], CloudQuery];
  assert.equal(sent[1], 'r1');
  assert.equal(sent[2].length, 405);
  assert.deepEqual(sent[3], query);
  assert.equal(state.error.value, '');
  assert.equal(state.busy.value, false);
  scope.stop();
});
test('empty results never open a download or mutate a queue; expired sessions return to account handling', async () => {
  for (const expired of [false, true]) {
    let actions = 0,
      sessions = 0;
    const scope = effectScope();
    const state = scope.run(() =>
      useCloudToolbar(
        {
          cloudPage: async () => {
            if (expired) throw { code: 'session_expired', message: '会话过期' };
            return page(0, 0);
          },
        },
        source,
        async () => {
          actions++;
          return true;
        },
        () => sessions++,
      ),
    )!;
    await state.run('queue');
    assert.equal(actions, 0);
    assert.equal(sessions, Number(expired));
    assert(state.error.value);
    scope.stop();
  }
});
