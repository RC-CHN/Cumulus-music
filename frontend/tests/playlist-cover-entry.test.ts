import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlaylistCoverEntry } from '../src/composables/usePlaylistCoverEntry.ts';
import type { AccountData, AccountView, CoreClient } from '../src/services/contracts.ts';
import type { CoverTask, CoverImage, PlaylistCoverClient } from '../src/services/playlistCover.ts';
import type { PlaylistEditor } from '../src/services/playlists.ts';
const data = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
const task = (phase: CoverTask['phase'] = 'draft'): CoverTask => ({
  id: 'test',
  playlistId: '3',
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
  crop: { x: 143, y: 63, size: 354 },
  preview: 'data:image/jpeg;base64,synthetic',
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function fixture(overrides: Partial<PlaylistCoverClient> = {}) {
  let account: AccountView = {
    epoch: '1',
    status: 'authenticated',
    nickname: 'test',
    avatarUrl: null,
  };
  const editor: PlaylistEditor = {
    id: '3',
    revision: 'revision',
    name: 'draft',
    description: 'unchanged',
    coverUrl: null,
    tags: [],
    owned: true,
    favorite: false,
    private: true,
    trackIds: [],
    hasLocal: false,
  };
  let picks = 0,
    focus = 0,
    expired = 0;
  const api: PlaylistCoverClient = {
    playlistCoverTasks: async () => data([]),
    playlistCoverImage: async () => data(image),
    pickPlaylistCover: async () => {
      picks++;
      return data(task());
    },
    playlistCoverCommand: async () =>
      data({ task: task('confirmed'), editor: null, warning: null }),
    ...overrides,
  };
  const entry = usePlaylistCoverEntry(
    () => account,
    () => editor,
    async () => {},
    () => {
      expired++;
    },
    async () => {
      assert.equal(entry.starting.value, false);
      focus++;
    },
    api as CoreClient,
  );
  return {
    entry,
    editor,
    picks: () => picks,
    focus: () => focus,
    expired: () => expired,
    account: (value: AccountView) => {
      account = value;
    },
  };
}
test('first selection stays on the page, ignores duplicate starts and cancellation restores focus', async () => {
  const pending = deferred<AccountData<CoverTask | null>>();
  let picks = 0;
  const f = fixture({
    pickPlaylistCover: () => {
      picks++;
      return pending.promise;
    },
  });
  const opening = f.entry.start();
  await new Promise((r) => setImmediate(r));
  assert.equal(f.entry.phase.value, 'picking');
  assert.equal(f.entry.open.value, false);
  await f.entry.start();
  assert.equal(picks, 1);
  pending.resolve(data(null));
  await opening;
  assert.equal(f.entry.open.value, false);
  assert.equal(f.entry.model.task.value, undefined);
  assert.equal(f.focus(), 1);
  assert.equal(f.editor.name, 'draft');
  f.entry.dispose();
});
test('cropper waits for the chosen image preview and uses the same model when it opens', async () => {
  const pending = deferred<AccountData<CoverImage>>();
  const f = fixture({ playlistCoverImage: () => pending.promise });
  const opening = f.entry.start();
  await new Promise((r) => setImmediate(r));
  assert.equal(f.entry.open.value, false);
  assert.equal(f.entry.starting.value, true);
  pending.resolve(data(image));
  await opening;
  assert.equal(f.entry.open.value, true);
  assert.equal(f.entry.model.image.value, image);
  assert.equal(f.picks(), 1);
  f.entry.dismiss();
  assert.equal(f.entry.open.value, false);
  f.entry.dispose();
});
test('task read failure and mismatched epochs never become permission to create a new task', async () => {
  for (const read of [
    async () => {
      throw { code: 'network', message: 'read failed' };
    },
    async () => data([], 'other'),
  ]) {
    const f = fixture({ playlistCoverTasks: read });
    await f.entry.start();
    assert.equal(f.entry.open.value, true);
    assert.equal(f.entry.model.readFailed.value, true);
    await f.entry.model.choose('revision');
    assert.equal(f.picks(), 0);
    f.entry.dispose();
  }
});
test('existing draft, pending bind and cleanup resume directly without selecting another file', async () => {
  for (const phase of ['draft', 'binding', 'confirmed'] as const) {
    const current = { ...task(phase), cleanupPending: phase === 'confirmed' };
    const f = fixture({ playlistCoverTasks: async () => data([current]) });
    await f.entry.start();
    assert.equal(f.entry.open.value, true);
    assert.equal(f.entry.model.task.value, current);
    assert.equal(f.picks(), 0);
    f.entry.dispose();
  }
  const f = fixture();
  await f.entry.start(true);
  assert.equal(f.entry.open.value, false);
  assert.equal(f.picks(), 0);
  assert.match(f.entry.notice.value, /没有需要恢复/);
  f.entry.dispose();
});
test('invalid selection opens visible recovery feedback without inventing an image or task', async () => {
  const f = fixture({
    pickPlaylistCover: async () => {
      throw { code: 'cover_dimensions', message: '请选择宽高至少 320 像素的图片' };
    },
  });
  await f.entry.start();
  assert.equal(f.entry.open.value, true);
  assert.match(f.entry.model.error.value, /320/);
  assert.equal(f.entry.model.task.value, undefined);
  assert.equal(f.entry.model.image.value, undefined);
  assert.equal(f.entry.model.readFailed.value, false);
  f.entry.dispose();
});
test('page disposal or account change while the native picker is open cannot reopen an old cropper', async () => {
  for (const mode of ['dispose', 'account']) {
    const pending = deferred<AccountData<CoverTask | null>>();
    const f = fixture({ pickPlaylistCover: () => pending.promise });
    const opening = f.entry.start();
    await new Promise((r) => setImmediate(r));
    if (mode === 'dispose') f.entry.dispose();
    else f.account({ epoch: '2', status: 'authenticated', nickname: 'other', avatarUrl: null });
    pending.resolve(data(task()));
    await opening;
    assert.equal(f.entry.open.value, false);
    assert.equal(f.focus(), 0);
    f.entry.dispose();
  }
});
test('session expiry during selection stops entry instead of showing an old error dialog', async () => {
  const f = fixture({
    pickPlaylistCover: async () => {
      throw { code: 'session_expired', message: 'expired' };
    },
  });
  await f.entry.start();
  assert.equal(f.expired(), 1);
  assert.equal(f.entry.open.value, false);
  assert.equal(f.entry.starting.value, false);
  f.entry.dispose();
});
