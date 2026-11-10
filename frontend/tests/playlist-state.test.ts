import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRenderer, defineComponent, h } from 'vue';
import { usePlaylistEditor } from '../src/composables/usePlaylistEditor.ts';
import { usePlaylists } from '../src/composables/usePlaylists.ts';
import {
  playlistLength,
  playlistSaved,
  type PlaylistClient,
  type PlaylistCommand,
  type PlaylistEditor,
  type PlaylistMixedAddTarget,
  type PlaylistMutationView,
} from '../src/services/playlists.ts';
import type { CoverOutcome } from '../src/services/playlistCover.ts';
import type { CoreClient, AccountData } from '../src/services/contracts.ts';
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
const data = <T>(value: T, epoch = '1'): AccountData<T> => ({ epoch, value });
const account = (epoch = '1') => ({
  epoch,
  status: 'authenticated' as const,
  nickname: 'test',
  avatarUrl: null,
});
const editor = (id = '10'): PlaylistEditor => ({
  id,
  revision: 'fresh',
  name: 'test',
  description: '',
  coverUrl: null,
  tags: [],
  owned: true,
  private: true,
  favorite: false,
  trackIds: ['1', '2', '3'],
  hasLocal: false,
});
const receipt = (phase: PlaylistMutationView['phase'] = 'confirmed'): PlaylistMutationView => ({
  operationId: 'op',
  kind: 'metadata',
  playlistId: '10',
  phase,
  label: 'test',
  serviceCodes: {},
  message: phase,
  observed: editor(),
});
const turn = () => new Promise((r) => setImmediate(r));
function fixture(
  overrides: Partial<PlaylistClient> = {},
  changed = async (_r: PlaylistMutationView) => {},
) {
  const core: PlaylistClient & Pick<CoreClient, 'playlistCoverTasks'> = {
    playlistCoverTasks: async () => data([]),
    playlistEditor: async (_e, id) => data(editor(id)),
    playlistOrderView: async () =>
      data({ revision: 'fresh', created: ['5', '10', '11'], subscribed: [] }),
    playlistTags: async () => data([]),
    playlistOperations: async () => data([]),
    mutatePlaylist: async () => data(receipt()),
    reconcilePlaylist: async () => data(receipt()),
    ...overrides,
  };
  const model = usePlaylists(core as CoreClient, changed);
  model.connect(account());
  return model;
}
test('old account writes and closed editor responses cannot replace current private views', async () => {
  const open = deferred<AccountData<PlaylistEditor>>(),
    write = deferred<AccountData<PlaylistMutationView>>();
  let changes = 0;
  const p = fixture(
    { playlistEditor: () => open.promise, mutatePlaylist: () => write.promise },
    async () => {
      changes++;
    },
  );
  const opening = p.open('10');
  p.reset();
  open.resolve(data(editor()));
  await opening;
  assert.equal(p.editor.value, undefined);
  const saving = p.mutate({ kind: 'create', name: 'test', private: true });
  p.connect(account('2'));
  write.resolve(data(receipt()));
  await saving;
  assert.equal(p.result.value, undefined);
  assert.equal(changes, 0);
  assert.equal(p.busy.value, false);
});
test('track ordering uses all authoritative IDs and stale preparation never sends a write', async () => {
  const sent: PlaylistCommand[] = [];
  const ids = Array.from({ length: 205 }, (_, i) => String(i + 1));
  const p = fixture({
    playlistEditor: async () => data({ ...editor(), trackIds: ids }),
    mutatePlaylist: async (_e, c) => {
      sent.push(c);
      return data(receipt());
    },
  });
  await p.tracks('10', 'down', '150');
  const command = sent[0]!;
  assert.equal(command.kind, 'tracks');
  if (command.kind !== 'tracks') throw new Error();
  assert.equal(command.ids.length, 205);
  assert.deepEqual(command.ids.slice(148, 152), ['149', '151', '150', '152']);
  assert.equal(command.revision, 'fresh');
  const old = deferred<AccountData<PlaylistEditor>>();
  const other = fixture({
    playlistEditor: () => old.promise,
    mutatePlaylist: async (_e, c) => {
      sent.push(c);
      return data(receipt());
    },
  });
  const moving = other.tracks('10', 'up', '150');
  other.connect(account('2'));
  old.resolve(data({ ...editor(), trackIds: ids }));
  await moving;
  assert.equal(sent.length, 1);
});
test('search collection reads a fresh revision, deduplicates IDs, and stops if the account changes', async () => {
  const sent: PlaylistCommand[] = [];
  const read = deferred<AccountData<PlaylistEditor>>();
  let delayed = false;
  const p = fixture({
    playlistEditor: async () => (delayed ? read.promise : data(editor())),
    mutatePlaylist: async (_e, command) => {
      sent.push(command);
      return data(receipt());
    },
  });
  await p.addTracks('10', ['1', '2', '1']);
  assert.deepEqual(sent, [
    { kind: 'tracks', id: '10', revision: 'fresh', change: 'add', ids: ['1', '2'] },
  ]);
  delayed = true;
  const pending = p.addTracks('10', ['3']);
  p.connect(account('2'));
  read.resolve(data(editor()));
  await pending;
  assert.equal(sent.length, 1);
});
test('an earlier mixed destination receipt is read back without submitting the current song', async () => {
  let writes = 0;
  const checks: unknown[][] = [];
  const pending = {
    playlistId: '10',
    operationId: 'earlier-op',
    phase: 'uncertain' as const,
    evidence: null,
    serviceCode: null,
  };
  const model = fixture({
    playlistMixedAddTarget: async (epoch, id, song) => {
      assert.deepEqual([epoch, id, song], ['1', '10', '704']);
      return data({ kind: 'pending', receipt: pending, currentSelection: false });
    },
    addCatalogSongToMixedPlaylist: async () => {
      writes++;
      return data(pending);
    },
    reconcilePlaylistMixedAdd: async (...args) => {
      checks.push(args);
      return data({ ...pending, phase: 'confirmed', evidence: 'changed_readback' });
    },
  });
  const earlier = await model.addTracks('10', ['704'], true);
  assert.equal(earlier?.mixedTrackAddition, 'earlier');
  assert.match(earlier!.message, /本次未提交/);
  const verified = await model.reconcileMixedAdd('10', earlier!.operationId, 'earlier');
  assert.equal(verified?.phase, 'confirmed');
  assert.equal(verified?.mixedTrackAddition, 'earlier');
  assert.match(verified!.message, /本次未提交/);
  assert.deepEqual(checks, [['1', '10', 'earlier-op']]);
  assert.equal(writes, 0);
});
test('a stale mixed destination preparation cannot send a write in another account', async () => {
  const wait = deferred<AccountData<PlaylistMixedAddTarget | null>>();
  let calls = 0;
  const model = fixture({
    playlistMixedAddTarget: () => wait.promise,
    addCatalogSongToMixedPlaylist: async () => {
      calls++;
      throw new Error('stale request was sent');
    },
  });
  const adding = model.addTracks('10', ['703'], true);
  model.connect(account('2'));
  wait.resolve(data({ kind: 'ready', revision: 'mixed', operationId: 'new-op' }));
  assert.equal(await adding, undefined);
  assert.equal(model.result.value, undefined);
  assert.equal(calls, 0);
});
test('unknown replies are not retried and explicit reconciliation only invokes the readback use case', async () => {
  let writes = 0,
    checks = 0;
  const hold = deferred<AccountData<PlaylistMutationView>>();
  const p = fixture({
    mutatePlaylist: () => {
      writes++;
      return hold.promise;
    },
    reconcilePlaylist: async () => {
      checks++;
      return data(receipt());
    },
  });
  const saving = p.mutate({ kind: 'create', name: 'test', private: true });
  await p.mutate({ kind: 'create', name: 'test', private: true });
  hold.reject({ code: 'network', message: 'lost response' });
  await saving;
  assert.equal(writes, 1);
  assert.equal(p.error.value, 'lost response');
  await p.reconcile('op');
  assert.equal(checks, 1);
  assert.equal(writes, 1);
});
test('partial metadata is exposed honestly and an earlier operation list cannot overwrite fresh reconciliation', async () => {
  const old = deferred<AccountData<PlaylistMutationView[]>>();
  let reads = 0;
  const p = fixture({
    playlistOperations: () => (++reads === 1 ? old.promise : Promise.resolve(data([]))),
    mutatePlaylist: async () =>
      data({
        ...receipt('partial'),
        serviceCodes: { name: 400, tags: 200, description: 200 },
        observed: { ...editor(), name: 'old' },
      }),
  });
  const value = await p.mutate({
    kind: 'metadata',
    id: '10',
    revision: 'fresh',
    name: 'new',
    tags: [],
    description: '',
  });
  assert.equal(value?.phase, 'partial');
  assert.equal(playlistSaved(value!.phase), false);
  assert.equal(p.editor.value?.name, 'old');
  old.resolve(data([receipt('uncertain')]));
  await turn();
  assert.deepEqual(p.operations.value, []);
  assert.equal(playlistLength('a中🎵'), 7);
});

test('tag catalogue distinguishes loading, empty and failed responses', async () => {
  const request = deferred<AccountData<{ name: string; category: string }[]>>();
  const model = fixture({ playlistTags: () => request.promise });
  const pending = model.loadTags();
  assert.equal(model.tagsLoading.value, true);
  request.resolve(data([]));
  await pending;
  assert.equal(model.tagsLoading.value, false);
  assert.equal(model.tagError.value, '');
  assert.deepEqual(model.tags.value, []);
  const failed = fixture({
    playlistTags: async () => {
      throw { code: 'network', message: '标签读取失败' };
    },
  });
  await failed.loadTags();
  assert.equal(failed.tagsLoading.value, false);
  assert.equal(failed.tagError.value, '标签读取失败');
});

test('late catalogue requests cannot overwrite a retry or another account', async () => {
  const old = deferred<AccountData<{ name: string; category: string }[]>>();
  let calls = 0;
  const tags = [{ name: '轻音乐', category: '风格' }];
  const model = fixture({
    playlistTags: () => (++calls === 1 ? old.promise : Promise.resolve(data(tags))),
  });
  const first = model.loadTags();
  await model.loadTags();
  old.reject({ code: 'network', message: '旧请求失败' });
  await first;
  assert.deepEqual(model.tags.value, tags);
  assert.equal(model.tagError.value, '');
  assert.equal(model.tagsLoading.value, false);
  const previous = deferred<AccountData<{ name: string; category: string }[]>>();
  const other = fixture({ playlistTags: () => previous.promise });
  const pending = other.loadTags();
  other.connect(account('2'));
  previous.resolve(data(tags));
  await pending;
  assert.deepEqual(other.tags.value, []);
  assert.equal(other.tagsLoading.value, false);
});

test('a stale editor reads latest without replaying and preserves the reviewed draft on explicit save', async () => {
  const original = { ...editor(), revision: 'old', tags: ['old'] };
  const latest = { ...original, revision: 'latest', name: 'remote', description: 'remote-only' };
  const writes: PlaylistCommand[] = [];
  let reads = 0;
  let closed = false;
  const p = fixture({
    playlistEditor: async () => data(++reads === 1 ? original : latest),
    mutatePlaylist: async (_epoch, command) => {
      writes.push(command);
      if (writes.length === 1) {
        throw { code: 'snapshot_changed', message: 'playlist changed', retryable: true };
      }
      return data(receipt());
    },
  });
  const { form, app } = mountEditor(p, () => (closed = true));
  try {
    await turn();
    assert.equal(form.loaded.value, true);
    form.name.value = 'local';
    form.selectedTags.value = ['local'];
    await form.save();
    assert.equal(form.needsRefresh.value, true);
    await form.save();
    assert.equal(writes.length, 1);
    assert.equal(reads, 1);

    await form.refreshDraft();
    assert.equal(reads, 2);
    assert.equal(writes.length, 1);
    assert.deepEqual(form.conflicts.value, ['name']);
    assert.equal(form.name.value, 'local');
    assert.equal(form.latestDraft.value.name, 'remote');
    assert.equal(form.description.value, 'remote-only');
    assert.deepEqual(form.selectedTags.value, ['local']);
    await form.save();
    assert.equal(writes.length, 1);

    form.resolveConflicts(true);
    assert.equal(writes.length, 1);
    await form.save();
    assert.deepEqual(writes[1], {
      kind: 'metadata',
      id: '10',
      revision: 'latest',
      name: 'local',
      description: 'remote-only',
      tags: ['local'],
    });
    assert.equal(closed, true);
  } finally {
    app.unmount();
  }
});

function mountEditor(p: ReturnType<typeof fixture>, close = () => {}, coverChanged = () => {}) {
  // Exercise the production form lifecycle without a browser.
  type Host = Record<string, never>;
  const empty = (): Host => ({});
  const renderer = createRenderer<Host, Host>({
    patchProp() {},
    insert() {},
    remove() {},
    createElement: empty,
    createText: empty,
    createComment: empty,
    setText() {},
    setElementText() {},
    parentNode: () => null,
    nextSibling: () => null,
  });
  let form!: ReturnType<typeof usePlaylistEditor>;
  const app = renderer.createApp(
    defineComponent({
      setup() {
        form = usePlaylistEditor(p, account, '10', close, coverChanged);
        return () => h('div');
      },
    }),
  );
  app.mount({});
  return { form, app };
}

function coverReceipt(editor: PlaylistEditor): CoverOutcome {
  return {
    editor,
    warning: null,
    task: {
      id: 'cover-task',
      playlistId: editor.id,
      phase: 'confirmed',
      crop: null,
      width: 400,
      height: 400,
      bytes: 100,
      total: 100,
      transfer: 'uploaded',
      binding: null,
      error: null,
      mayLeaveObject: false,
      cleanupPending: false,
    },
  };
}

for (const remoteEdit of [false, true]) {
  test(
    remoteEdit
      ? 'cover readback with changed metadata requires review before saving'
      : 'cover-only revision lets the existing draft save without a false conflict',
    async () => {
      const original = { ...editor(), revision: 'before-cover' };
      const latest = {
        ...original,
        revision: 'after-cover',
        coverUrl: 'https://example.invalid/synthetic',
        ...(remoteEdit ? { name: 'remote', description: 'remote-only' } : {}),
      };
      let reads = 0;
      let coverUpdates = 0;
      const writes: PlaylistCommand[] = [];
      const p = fixture({
        playlistEditor: async () => data(++reads === 1 ? original : latest),
        mutatePlaylist: async (_epoch, command) => {
          writes.push(command);
          return data(receipt());
        },
      });
      const { app, form } = mountEditor(
        p,
        () => {},
        () => coverUpdates++,
      );
      try {
        await turn();
        form.name.value = 'local';
        await form.coverSaved(coverReceipt(latest));
        assert.equal(coverUpdates, 1);
        assert.equal(reads, 1);
        assert.equal(form.name.value, 'local');
        assert.equal(form.needsRefresh.value, remoteEdit);
        if (remoteEdit) {
          await form.save();
          assert.equal(writes.length, 0);
          await form.refreshDraft();
          assert.equal(reads, 2);
          assert.deepEqual(form.conflicts.value, ['name']);
          assert.equal(form.latestDraft.value.name, 'remote');
          assert.equal(form.description.value, 'remote-only');
          form.resolveConflicts(false);
          assert.equal(form.name.value, 'remote');
          form.name.value = 'local-after-review';
        }
        assert.equal(writes.length, 0);
        await form.save();
        assert.deepEqual(writes, [
          {
            kind: 'metadata',
            id: '10',
            revision: 'after-cover',
            name: remoteEdit ? 'local-after-review' : 'local',
            description: remoteEdit ? 'remote-only' : '',
            tags: [],
          },
        ]);
      } finally {
        app.unmount();
      }
    },
  );
}

test('partial metadata readback permits explicit retry at the observed revision', async () => {
  const original = { ...editor(), revision: 'original' };
  const partial = { ...original, revision: 'partial-readback', name: 'local' };
  const writes: PlaylistCommand[] = [];
  let closed = false;
  const p = fixture({
    playlistEditor: async () => data(original),
    mutatePlaylist: async (_epoch, command) => {
      writes.push(command);
      return data(writes.length === 1 ? { ...receipt('partial'), observed: partial } : receipt());
    },
  });
  const { app, form } = mountEditor(p, () => (closed = true));
  try {
    await turn();
    form.name.value = 'local';
    form.description.value = 'remaining';
    await form.save();
    assert.equal(writes.length, 1);
    assert.equal(form.refreshBlocked.value, false);
    assert.equal(form.description.value, 'remaining');
    await form.save();
    assert.equal(writes.length, 2);
    assert.equal('revision' in writes[1]! && writes[1].revision, 'partial-readback');
    assert.equal(closed, true);
  } finally {
    app.unmount();
  }
});
