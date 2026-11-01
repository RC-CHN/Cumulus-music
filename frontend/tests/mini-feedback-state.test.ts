import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, nextTick, shallowRef } from 'vue';
import { useMiniFeedback } from '../src/composables/useMiniFeedback.ts';
import type { MiniClient, MiniView, MiniFmFeedbackView } from '../src/services/mini.ts';
import type { FmReasons } from '../src/services/fm.ts';
const turn = () => new Promise<void>((r) => setImmediate(r));
function deferred<T>() {
  let resolve!: (v: T) => void, reject!: (e: unknown) => void;
  const promise = new Promise<T>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { promise, resolve, reject };
}
function fixture(overrides: Partial<MiniClient> = {}) {
  const view = shallowRef<MiniView | null>({
    generation: '1',
    ticket: '1',
    preferences: { revision: 'p', alwaysOnTop: true, damaged: false },
    pinApplied: true,
    media: {
      epoch: '1',
      selection: { audioPlayId: 'play-1', audioEntryId: 'entry-1', videoPlayId: null },
      item: { id: '101', kind: 'music', title: 'Original', artists: [], album: '', artwork: null },
      state: 'paused',
      playing: false,
      canPrevious: true,
      canNext: true,
      position: 10,
      duration: 30,
      seekStart: 0,
      seekEnd: 30,
      canSeek: true,
      volume: 100,
      mode: 'sequential',
      modeFamily: 'fm',
      queueRevision: 'q',
      like: null,
      error: null,
    },
  });
  const status: MiniFmFeedbackView = {
    epoch: '1',
    entryId: 'entry-1',
    title: 'Original',
    pending: false,
  };
  const offered: FmReasons = {
    entryId: 'entry-1',
    songId: '101',
    options: [{ key: 'artist-1', label: 'Artist', artistId: '1' }],
  };
  const writes: unknown[] = [],
    reads: unknown[] = [];
  let reasonReads = 0,
    refreshes = 0;
  const client = {
    applyPreferences: async () => {},
    lyricMenu: async () => {
      throw Error('unused');
    },
    lyricMenuAction: async () => {},
    exit: async () => {},
    feedbackView: async (ticket: string, remote: boolean) => {
      reads.push([ticket, remote]);
      return { ...status };
    },
    feedbackReasons: async () => {
      reasonReads++;
      return offered;
    },
    feedbackSubmit: async (ticket: string, key?: string) => {
      writes.push([ticket, key]);
    },
    ...overrides,
  } as MiniClient;
  const scope = effectScope();
  const model = scope.run(() =>
    useMiniFeedback(view, client, async () => {
      refreshes++;
      return view.value ?? undefined;
    }),
  )!;
  const update = (epoch = '1', entryId = 'entry-1', ticket = '2') => {
    const old = view.value!;
    view.value = {
      ...old,
      ticket,
      media: { ...old.media, epoch, selection: { ...old.media.selection, audioEntryId: entryId } },
    };
  };
  return {
    view,
    status,
    offered,
    writes,
    reads,
    scope,
    model,
    update,
    reasonReads: () => reasonReads,
    refreshes: () => refreshes,
  };
}

test('mini feedback waits for native observation, then binds only actual offered reason keys', async (t) => {
  const pending = deferred<MiniFmFeedbackView>();
  const f = fixture({ feedbackView: () => pending.promise });
  t.after(() => f.scope.stop());
  const opening = f.model.show();
  assert.equal(f.model.canSubmit.value, false);
  pending.resolve(f.status);
  await opening;
  await turn();
  assert.equal(f.model.canSubmit.value, true);
  assert.equal(await f.model.submit('forged'), false);
  assert.equal(f.writes.length, 0);
  assert.equal(await f.model.submit('artist-1'), true);
  assert.deepEqual(f.writes, [['1', 'artist-1']]);
  assert.equal(f.model.open.value, false);
});

test('reason failures allow song-only feedback and closing rejects late reason data', async (t) => {
  let calls = 0;
  const late = deferred<FmReasons>();
  const f = fixture({
    feedbackReasons: async () => {
      if (++calls === 1) throw { code: 'network', message: 'Reasons failed' };
      return late.promise;
    },
  });
  t.after(() => f.scope.stop());
  await f.model.show();
  await turn();
  assert.equal(f.model.reasonIssue.value?.message, 'Reasons failed');
  assert.equal(f.model.canSubmit.value, true);
  const reading = f.model.loadReasons();
  f.model.close();
  late.resolve(f.offered);
  await reading;
  assert.equal(f.model.reasons.value, undefined);
  assert.equal(f.model.loadingReasons.value, false);
});

test('unknown write cannot resend; readonly status can reveal a journal without applying it', async (t) => {
  let writes = 0;
  const f = fixture({
    feedbackSubmit: async () => {
      writes++;
      throw { code: 'write_uncertain', message: 'Inspect first' };
    },
  });
  t.after(() => f.scope.stop());
  await f.model.show();
  await turn();
  await f.model.submit();
  assert.equal(f.model.open.value, true);
  assert.equal(f.model.issue.value?.message, 'Inspect first');
  assert.equal(f.model.canSubmit.value, false);
  assert.equal(f.refreshes(), 1);
  await f.model.submit();
  assert.equal(writes, 1);
  assert.equal(f.reads.length, 1);
  f.status.pending = true;
  f.update();
  await nextTick();
  await f.model.readState();
  assert.deepEqual(f.reads.at(-1), ['2', true]);
  assert.equal(f.model.state.value?.pending, true);
  assert.equal(f.model.canSubmit.value, false);
  await f.model.submit();
  assert.equal(writes, 1);
});

test('known rejection requires explicit successful reread before the next deliberate attempt', async (t) => {
  let writes = 0;
  const f = fixture({
    feedbackSubmit: async () => {
      if (++writes === 1) throw { code: 'service_rejected', message: 'Refused' };
    },
  });
  t.after(() => f.scope.stop());
  await f.model.show();
  await turn();
  await f.model.submit();
  assert.equal(f.model.canSubmit.value, false);
  f.update();
  await nextTick();
  await f.model.readState();
  assert.equal(f.model.canSubmit.value, true);
  assert.equal(f.model.issue.value?.message, 'Refused');
  assert.equal(writes, 1);
  assert.equal(await f.model.submit(), true);
  assert.equal(writes, 2);
});

test('initial state read failure never exposes a writable old menu, and failed rereads remain blocked', async (t) => {
  let broken = true;
  const f = fixture({
    feedbackView: async () => {
      if (broken) throw { code: 'storage', message: 'Unreadable' };
      return f.status;
    },
  });
  t.after(() => f.scope.stop());
  await f.model.show();
  assert.equal(f.model.canSubmit.value, false);
  assert.equal(f.reasonReads(), 0);
  await f.model.readState();
  assert.equal(f.model.canSubmit.value, false);
  broken = false;
  await f.model.readState();
  assert.equal(f.model.canSubmit.value, true);
  assert.equal(f.writes.length, 0);
});

test('a popup never recaptures another entry after a partial artist failure', async (t) => {
  const f = fixture({
    feedbackSubmit: async () => {
      f.update('1', 'entry-2');
      throw { code: 'artist_rejected', message: 'Song accepted, artist refused' };
    },
  });
  t.after(() => f.scope.stop());
  await f.model.show();
  await turn();
  await f.model.submit('artist-1');
  assert.equal(f.model.target.value?.entryId, 'entry-1');
  assert.equal(f.model.target.value?.title, 'Original');
  assert.equal(f.model.sameEntry.value, false);
  assert.equal(f.model.canSubmit.value, false);
  const reads = f.reads.length;
  assert.equal(await f.model.readState(), false);
  assert.equal(f.reads.length, reads);
});

test('account changes invalidate late local status and late write success', async (t) => {
  const late = deferred<MiniFmFeedbackView>();
  const f = fixture({ feedbackView: () => late.promise });
  t.after(() => f.scope.stop());
  const opening = f.model.show();
  f.update('2');
  await nextTick();
  late.resolve(f.status);
  await opening;
  assert.equal(f.model.open.value, false);
  assert.equal(f.model.state.value, undefined);
  const success = deferred<void>();
  const g = fixture({ feedbackSubmit: () => success.promise });
  t.after(() => g.scope.stop());
  await g.model.show();
  await turn();
  const writing = g.model.submit();
  g.update('2');
  await nextTick();
  success.resolve();
  assert.equal(await writing, false);
  assert.equal(g.model.open.value, false);
});

test('native unresolved feedback suppresses reasons and writes, including a fabricated UI attempt', async (t) => {
  const f = fixture();
  f.status.pending = true;
  t.after(() => f.scope.stop());
  await f.model.show();
  await turn();
  assert.equal(f.model.canSubmit.value, false);
  assert.equal(f.reasonReads(), 0);
  assert.equal(await f.model.submit(), false);
  assert.equal(f.writes.length, 0);
});
