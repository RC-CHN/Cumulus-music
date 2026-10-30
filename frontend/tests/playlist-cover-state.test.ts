import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlaylistCover } from '../src/composables/usePlaylistCover.ts';
import {
  coverCrop,
  coverPosition,
  coverZoomLimits,
  dragCover,
  restoreCoverPosition,
} from '../src/services/coverGeometry.ts';
import type {
  PlaylistCoverClient,
  CoverTask,
  CoverOutcome,
  CoverCommand,
  CoverImage,
} from '../src/services/playlistCover.ts';
import type { AccountData, CoreClient } from '../src/services/contracts.ts';

const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: 'test',
  avatarUrl: null,
});
const data = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
const task = (phase: CoverTask['phase'] = 'draft'): CoverTask => ({
  id: 'task',
  playlistId: '10',
  phase,
  crop: null,
  width: 640,
  height: 480,
  bytes: 0,
  total: 0,
  transfer: null,
  binding: null,
  error: null,
  mayLeaveObject: false,
  cleanupPending: false,
});
const image: CoverImage = {
  width: 640,
  height: 480,
  crop: { x: 80, y: 0, size: 480 },
  preview: 'data:image/jpeg;base64,fixture',
};
const outcome = (phase: CoverTask['phase'] = 'confirmed'): CoverOutcome => ({
  task: task(phase),
  editor: null,
  warning: null,
});
function deferred<T>() {
  let resolve!: (v: T) => void, reject!: (e: unknown) => void;
  const promise = new Promise<T>((a, b) => {
    resolve = a;
    reject = b;
  });
  return { promise, resolve, reject };
}
function fixture(
  overrides: Partial<PlaylistCoverClient> = {},
  saved: (v: CoverOutcome) => Promise<void> = async () => {},
  expired = () => {},
) {
  const core: PlaylistCoverClient = {
    playlistCoverTasks: async () => data([]),
    playlistCoverImage: async () => data(image),
    pickPlaylistCover: async () => data(task()),
    playlistCoverCommand: async () => data(outcome()),
    ...overrides,
  };
  return usePlaylistCover(core as CoreClient, saved, expired);
}
test('official viewport crop geometry preserves original pixels through drag, zoom and recovery', () => {
  const initial = coverPosition(640, 480);
  assert.deepEqual(coverCrop(640, 480, initial), { x: 143, y: 63, size: 354 });
  const min = coverZoomLimits(640, 480).min;
  assert.deepEqual(coverCrop(640, 480, coverPosition(640, 480, { ...initial, zoom: min })), {
    x: 80,
    y: 0,
    size: 480,
  });
  const selected = { x: 320, y: 40, size: 320 };
  const restored = restoreCoverPosition(640, 480, selected);
  assert.deepEqual(coverCrop(640, 480, restored), selected);
  assert.deepEqual(coverCrop(640, 480, dragCover(640, 480, restored, 10000, 10000)), {
    x: 0,
    y: 0,
    size: 320,
  });
  for (const [width, height] of [
    [320, 320],
    [480, 640],
    [16000, 320],
    [320, 10000],
  ]) {
    const p = coverPosition(width!, height!, { x: -10000, y: 100000, zoom: 100000 });
    const c = coverCrop(width!, height!, p);
    assert.ok(
      c.size >= 1 && c.x >= 0 && c.y >= 0 && c.x + c.size <= width! && c.y + c.size <= height!,
    );
  }
});
test('native picker cancellation keeps the selected image and replacement refers to its existing task', async () => {
  const calls: (string | null)[] = [];
  let count = 0;
  const model = fixture({
    pickPlaylistCover: async (_e, _p, _r, id) => {
      calls.push(id);
      return data(++count === 1 ? task() : null);
    },
  });
  await model.connect(account(), '10');
  await model.choose('revision');
  const selected = model.image.value;
  await model.choose('revision');
  assert.deepEqual(calls, [null, 'task']);
  assert.equal(model.image.value, selected);
  assert.equal(model.task.value?.id, 'task');
  model.dispose();
});
test('late preview cannot leak a previous account image or replace the new loading state', async () => {
  const pending = deferred<AccountData<CoverImage>>();
  const model = fixture({
    playlistCoverTasks: async (e) => data(e === '1' ? [task()] : [], e),
    playlistCoverImage: () => pending.promise,
  });
  const opening = model.connect(account(), '10');
  await new Promise((r) => setImmediate(r));
  await model.connect(account('2'), '20');
  pending.resolve(data(image));
  await opening;
  assert.equal(model.task.value, undefined);
  assert.equal(model.image.value, undefined);
  assert.equal(model.loading.value, false);
  model.dispose();
});
test('preview retry clears its failure without hiding an independent save error', async () => {
  let failPreview = true;
  const model = fixture({
    playlistCoverTasks: async () => data([task()]),
    playlistCoverImage: async () => {
      if (failPreview) throw { code: 'media', message: 'preview unavailable' };
      return data(image);
    },
    playlistCoverCommand: async () => {
      throw { code: 'network', message: 'save failed' };
    },
  });
  await model.connect(account(), '10');
  assert.equal(model.error.value, 'preview unavailable');
  failPreview = false;
  await model.preview();
  assert.equal(model.error.value, '');
  assert.equal(model.image.value, image);
  await model.command({ kind: 'continue' });
  await model.preview();
  assert.equal(model.error.value, 'save failed');
  model.dispose();
});
test('preview retry is single-flight and late rejection cannot replace a new account preview', async () => {
  const old = deferred<AccountData<CoverImage>>();
  const fresh = deferred<AccountData<CoverImage>>();
  let reads = 0;
  const model = fixture({
    playlistCoverTasks: async (e) => data([task()], e),
    playlistCoverImage: async (e) => {
      reads++;
      if (reads === 1) throw { code: 'media', message: 'preview unavailable' };
      return e === '1' ? old.promise : fresh.promise;
    },
  });
  await model.connect(account(), '10');
  const retry = model.preview();
  const duplicate = model.preview();
  assert.equal(reads, 2);
  await duplicate;
  assert.equal(model.previewing.value, true);
  const opening = model.connect(account('2'), '10');
  await new Promise((r) => setImmediate(r));
  old.reject({ code: 'media', message: 'old preview failure' });
  await retry;
  assert.equal(model.previewing.value, true);
  assert.equal(model.error.value, '');
  fresh.resolve(data(image, '2'));
  await opening;
  assert.equal(model.previewing.value, false);
  assert.equal(model.image.value, image);
  model.dispose();
});
test('unknown save is recovered through a read-only task refresh and explicit reconcile, without a second save', async () => {
  const commands: string[] = [];
  let saved = 0,
    current = task();
  const model = fixture(
    {
      playlistCoverTasks: async () => data([current]),
      playlistCoverCommand: async (_e, _id, command) => {
        commands.push(command.kind);
        if (command.kind === 'save') {
          current = task('binding');
          throw { code: 'network', message: 'reply lost' };
        }
        current = task('confirmed');
        return data(outcome());
      },
    },
    async () => {
      saved++;
    },
  );
  await model.connect(account(), '10');
  await model.command({ kind: 'save', crop: image.crop });
  assert.equal(model.task.value?.phase, 'binding');
  await model.refresh();
  assert.deepEqual(commands, ['save']);
  await model.command({ kind: 'reconcile' });
  await model.command({ kind: 'reconcile' });
  assert.deepEqual(commands, ['save', 'reconcile', 'reconcile']);
  assert.equal(saved, 1);
  assert.equal(model.error.value, '');
  model.dispose();
});
test('a second save is ignored while busy and late completion after switching account is not published', async () => {
  const pending = deferred<AccountData<CoverOutcome>>();
  let writes = 0,
    saved = 0;
  const model = fixture(
    {
      playlistCoverTasks: async (e) => data(e === '1' ? [task()] : [], e),
      playlistCoverCommand: async () => {
        writes++;
        return pending.promise;
      },
    },
    async () => {
      saved++;
    },
  );
  await model.connect(account(), '10');
  const saving = model.command({ kind: 'save', crop: image.crop });
  await model.command({ kind: 'save', crop: image.crop });
  assert.equal(writes, 1);
  await model.connect(account('2'), '20');
  pending.resolve(data(outcome()));
  await saving;
  assert.equal(saved, 0);
  assert.equal(model.task.value, undefined);
  assert.equal(model.busy.value, false);
  model.dispose();
});
test('session expiry clears image and stops all follow-up reads and writes', async () => {
  let reads = 0,
    writes = 0,
    expired = 0;
  const model = fixture(
    {
      playlistCoverTasks: async () => {
        reads++;
        return data([task()]);
      },
      playlistCoverCommand: async () => {
        writes++;
        throw { code: 'session_expired', message: 'expired' };
      },
    },
    undefined,
    () => {
      expired++;
    },
  );
  await model.connect(account(), '10');
  await model.command({ kind: 'save', crop: image.crop });
  await model.refresh();
  await model.command({ kind: 'continue' });
  assert.equal(reads, 1);
  assert.equal(writes, 1);
  assert.equal(expired, 1);
  assert.equal(model.task.value, undefined);
  assert.equal(model.image.value, undefined);
  model.dispose();
});
test('revision conflict stays pending until an explicit rebase and never automatically retries', async () => {
  const commands: CoverCommand[] = [];
  let current = task('uploaded');
  const model = fixture({
    playlistCoverTasks: async () => data([current]),
    playlistCoverCommand: async (_e, _id, c) => {
      commands.push(c);
      if (c.kind === 'continue') {
        current = { ...current, error: 'playlist_changed' };
        throw { code: 'snapshot_changed', message: 'changed' };
      }
      current = { ...current, error: null };
      return data({ ...outcome('uploaded'), task: current });
    },
  });
  await model.connect(account(), '10');
  await model.command({ kind: 'continue' });
  assert.equal(model.changedElsewhere.value, true);
  await model.command({ kind: 'rebase', revision: 'reviewed-new-revision' });
  assert.equal(model.changedElsewhere.value, false);
  assert.deepEqual(commands, [
    { kind: 'continue' },
    { kind: 'rebase', revision: 'reviewed-new-revision' },
  ]);
  model.dispose();
});

test('pause stays pending until the active transfer settles and rejects duplicate pause clicks', async () => {
  const pending = deferred<AccountData<CoverOutcome>>();
  let pauses = 0;
  const current = { ...task('uploading'), transfer: 'uploading' as const, total: 500000 };
  const model = fixture({
    playlistCoverTasks: async () => data([current]),
    playlistCoverCommand: async (_e, _id, command) => {
      if (command.kind === 'pause') {
        pauses++;
        return data({ ...outcome('uploading'), task: current });
      }
      return pending.promise;
    },
  });
  await model.connect(account(), '10');
  const running = model.command({ kind: 'continue' });
  await model.pause();
  assert.equal(model.stopping.value, true);
  assert.equal(model.busy.value, true);
  await model.pause();
  assert.equal(pauses, 1);
  pending.resolve(data({ ...outcome('uploading'), task: { ...current, transfer: 'paused' } }));
  await running;
  assert.equal(model.stopping.value, false);
  assert.equal(model.busy.value, false);
  assert.equal(model.task.value?.transfer, 'paused');
  model.dispose();
});

test('failed pause is retryable while the original transfer is still pending', async () => {
  const pending = deferred<AccountData<CoverOutcome>>();
  const model = fixture({
    playlistCoverTasks: async () => data([task('uploading')]),
    playlistCoverCommand: async (_e, _id, command) => {
      if (command.kind === 'pause') throw { code: 'network', message: 'pause failed' };
      return pending.promise;
    },
  });
  await model.connect(account(), '10');
  const running = model.command({ kind: 'continue' });
  await model.pause();
  assert.equal(model.stopping.value, false);
  assert.equal(model.busy.value, true);
  assert.equal(model.error.value, 'pause failed');
  pending.resolve(data(outcome()));
  await running;
  model.dispose();
});

test('a late pause acknowledgment cannot replace a confirmed save', async () => {
  const running = deferred<AccountData<CoverOutcome>>();
  const pausing = deferred<AccountData<CoverOutcome>>();
  const model = fixture({
    playlistCoverTasks: async () => data([task('uploading')]),
    playlistCoverCommand: (_e, _id, command) =>
      command.kind === 'pause' ? pausing.promise : running.promise,
  });
  await model.connect(account(), '10');
  const save = model.command({ kind: 'continue' });
  const pause = model.pause();
  running.resolve(data(outcome()));
  await save;
  pausing.resolve(data(outcome('uploading')));
  await pause;
  assert.equal(model.task.value?.phase, 'confirmed');
  assert.equal(model.stopping.value, false);
  model.dispose();
});
