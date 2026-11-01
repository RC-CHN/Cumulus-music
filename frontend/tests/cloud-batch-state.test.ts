import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, ref } from 'vue';
import { readCloudBatch } from '../src/services/cloudBatch.ts';
import { useCloudBatch } from '../src/composables/useCloudBatch.ts';
import type { CloudClient, CloudPage, CloudTrack } from '../src/services/cloud.ts';
import type { AccountView } from '../src/services/contracts.ts';
function rows(n: number): CloudTrack[] {
  return Array.from({ length: n }, (_, i) => ({
    track: {
      id: String(i + 1),
      name: '合成',
      artists: [],
      album: '',
      coverUrl: null,
      durationMs: 1000,
      fee: 0,
      missing: false,
      local: false,
    },
    fileName: `${i}.flac`,
    fileSize: 12,
    addedAt: 1,
    bitrate: 1,
    format: 'flac',
  }));
}
function page(offset = 0, total = 405, revision = 'r1'): CloudPage {
  return {
    epoch: '1',
    revision,
    updatedMs: 1,
    size: 12,
    maxSize: 100,
    uploadedCount: total,
    uploadingCount: 0,
    total,
    offset,
    tracks: rows(total).slice(offset, offset + 200),
    uploads: [],
    deletions: [],
    workerError: null,
  };
}
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { resolve, promise };
}
test('cloud batch reads 200/200/5 in natural full-library order and refreshes once', async () => {
  const calls: unknown[][] = [],
    progress: number[] = [];
  const data = await readCloudBatch(
    {
      cloudPage: async (e, q, r) => {
        calls.push([e, q, r]);
        return page(q.offset);
      },
    },
    '1',
    () => true,
    (n) => progress.push(n),
  );
  assert.equal(data.tracks.length, 405);
  assert.deepEqual(progress, [200, 400, 405]);
  assert.deepEqual(
    calls.map((v) => v[2]),
    [true, false, false],
  );
  for (const v of calls) {
    const q = v[1] as { sort: string; keyword: string };
    assert.equal(q.sort, 'natural');
    assert.equal(q.keyword, '');
  }
  assert.equal(data.tracks.at(-1)?.track.id, '405');
});
test('changed revisions, duplicate rows, short pages and cancelled owners cannot become full batch data', async () => {
  for (const mode of ['revision', 'duplicate', 'empty', 'offset', 'epoch']) {
    await assert.rejects(
      readCloudBatch(
        {
          cloudPage: async (_e, q) => {
            const p = page(q.offset);
            if (q.offset) {
              if (mode === 'revision') p.revision = 'changed';
              if (mode === 'duplicate') p.tracks[0] = rows(1)[0]!;
              if (mode === 'empty') p.tracks = [];
              if (mode === 'offset') p.offset = 0;
              if (mode === 'epoch') p.epoch = '2';
            }
            return p;
          },
        },
        '1',
        () => true,
        () => {},
      ),
    );
  }
});
test('all selects beyond the first page, range selection and reload preserve only still-present IDs', async () => {
  let total = 405;
  const account = ref<AccountView>({
    epoch: '1',
    status: 'authenticated',
    nickname: null,
    avatarUrl: null,
  });
  const scope = effectScope();
  const model = scope.run(() =>
    useCloudBatch(
      { cloudPage: async (_e, q) => page(q.offset, total) } as CloudClient,
      () => account.value,
      () => {},
    ),
  )!;
  await model.load();
  model.all();
  assert.equal(model.chosen.value.length, 405);
  model.all();
  model.toggle(198);
  model.toggle(202, true);
  assert.deepEqual(
    model.chosen.value.map((v) => v.track.id),
    ['199', '200', '201', '202', '203'],
  );
  total = 201;
  await model.load();
  assert.deepEqual(
    model.chosen.value.map((v) => v.track.id),
    ['199', '200', '201'],
  );
  scope.stop();
});
test('account replacement and disposal discard a pending full-library read and issue no extra pages', async () => {
  for (const dispose of [false, true]) {
    const held = deferred<CloudPage>(),
      calls: number[] = [];
    const account = ref<AccountView>({
      epoch: '1',
      status: 'authenticated',
      nickname: null,
      avatarUrl: null,
    });
    const scope = effectScope();
    const model = scope.run(() =>
      useCloudBatch(
        {
          cloudPage: async (_e, q) => {
            calls.push(q.offset);
            return held.promise;
          },
        } as CloudClient,
        () => account.value,
        () => {},
      ),
    )!;
    const pending = model.load();
    if (dispose) scope.stop();
    else account.value = { ...account.value, epoch: '2' };
    held.resolve(page());
    await pending;
    assert.equal(model.data.value, undefined);
    assert.deepEqual(calls, [0]);
    model.all();
    assert.equal(model.selected.value.size, 0);
    scope.stop();
  }
});
test('a completed private library and selection clear when the owner changes', async () => {
  const account = ref<AccountView>({
    epoch: '1',
    status: 'authenticated',
    nickname: null,
    avatarUrl: null,
  });
  let calls = 0;
  const scope = effectScope();
  const model = scope.run(() =>
    useCloudBatch(
      {
        cloudPage: async (_e, q) => {
          calls++;
          return page(q.offset, 2);
        },
      } as CloudClient,
      () => account.value,
      () => {},
    ),
  )!;
  await model.load();
  model.all();
  assert.equal(model.chosen.value.length, 2);
  account.value = { ...account.value, epoch: '2' };
  await nextTick();
  assert.equal(model.data.value, undefined);
  assert.equal(model.selected.value.size, 0);
  await model.load();
  model.all();
  assert.equal(calls, 1);
  assert.equal(model.chosen.value.length, 0);
  scope.stop();
});
