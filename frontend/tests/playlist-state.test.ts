import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlaylists } from '../src/composables/usePlaylists.ts';
import {
  playlistLength,
  playlistSaved,
  type PlaylistClient,
  type PlaylistCommand,
  type PlaylistEditor,
  type PlaylistMutationView,
} from '../src/services/playlists.ts';
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
