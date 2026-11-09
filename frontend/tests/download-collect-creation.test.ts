import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useDownloadCollectCreation } from '../src/composables/useDownloadCollectCreation.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type {
  DownloadCollectCreation,
  DownloadCollectCreationCommand,
} from '../src/services/downloads.ts';
import type { PlaylistMutationView } from '../src/services/playlists.ts';

const owner = { epoch: '4', receipt: 'native-visit', canCreate: true };
const operation = (
  kind: 'create' | 'tracks',
  phase: PlaylistMutationView['phase'],
): PlaylistMutationView => ({
  operationId: kind === 'create' ? 'created-journal' : 'add-journal',
  kind,
  playlistId: phase === 'uncertain' && kind === 'create' ? null : '91',
  phase,
  label: '合成歌单',
  message: '',
  observed: null,
  serviceCodes: {},
});
const reply = (
  create: PlaylistMutationView,
  add: PlaylistMutationView | null = null,
  canAdd = false,
) => ({
  epoch: owner.epoch,
  value: { receipt: owner.receipt, create, add, canAdd },
});

test('a lost create can be read back without inventing add authority or sending create twice', async () => {
  const calls: DownloadCollectCreationCommand[] = [];
  let updates = 0;
  const model = useDownloadCollectCreation(
    {
      async downloadCollectCreation(epoch, receipt, command) {
        assert.deepEqual([epoch, receipt], [owner.epoch, owner.receipt]);
        calls.push(command);
        if (command.kind === 'create') throw { code: 'network', message: 'synthetic lost IPC' };
        if (command.kind === 'status')
          return reply(operation('create', command.reconcile ? 'confirmed' : 'uncertain'));
        return assert.fail('no explicit continue may be offered without the create receipt');
      },
    },
    () => owner,
    () => updates++,
    () => {},
    () => assert.fail('not expired'),
  );
  assert.deepEqual(calls, []);
  await model.create('合成歌单', false);
  assert.equal(model.phase.value, 'pending');
  await model.create('合成歌单', false);
  await model.continueAdd();
  await model.reconcile();
  assert.equal(model.phase.value, 'failed');
  assert.match(model.notice.value, /缺少创建回执/);
  assert.equal(updates, 1);
  assert.deepEqual(calls, [
    { kind: 'create', name: '合成歌单', privatePlaylist: false },
    { kind: 'status', reconcile: false },
    { kind: 'status', reconcile: true },
  ]);
});

test('a verified first add can be explicitly continued, and an account switch ignores a late result', async () => {
  let active: typeof owner | undefined = owner;
  const calls: DownloadCollectCreationCommand[] = [];
  let finish!: (result: AccountData<DownloadCollectCreation>) => void;
  const held = new Promise<AccountData<DownloadCollectCreation>>((resolve) => {
    finish = resolve;
  });
  let updates = 0;
  const model = useDownloadCollectCreation(
    {
      async downloadCollectCreation(_epoch, _receipt, command) {
        calls.push(command);
        if (command.kind === 'create') return reply(operation('create', 'uncertain'));
        if (command.kind === 'status') return reply(operation('create', 'confirmed'), null, true);
        return held;
      },
    },
    () => active,
    () => updates++,
    () => {},
    () => assert.fail('not expired'),
  );
  await model.create('合成歌单', true);
  await model.reconcile();
  assert.equal(model.phase.value, 'created_ready');
  assert.equal(updates, 1);
  const inFlight = model.continueAdd();
  active = undefined;
  finish(reply(operation('create', 'confirmed'), operation('tracks', 'confirmed')));
  await inFlight;
  assert.equal(updates, 1);
  assert.equal(model.phase.value, 'created_ready');
  assert.deepEqual(
    calls.map(({ kind }) => kind),
    ['create', 'status', 'add_created'],
  );
});

test('a definitive preflight failure allows a new explicit confirmation without a status read', async () => {
  const calls: DownloadCollectCreationCommand[] = [];
  const model = useDownloadCollectCreation(
    {
      async downloadCollectCreation(_epoch, _receipt, command) {
        calls.push(command);
        assert.equal(command.kind, 'create');
        if (calls.length === 1) {
          throw { code: 'download_create_not_started', message: '创建尚未发送，可以重试' };
        }
        return reply(operation('create', 'confirmed'), operation('tracks', 'confirmed'));
      },
    },
    () => owner,
    () => {},
    () => {},
    () => assert.fail('not expired'),
  );
  await model.create('合成歌单', false);
  assert.equal(model.phase.value, 'prepared');
  assert.equal(model.locked.value, false);
  assert.match(model.notice.value, /尚未发送/);
  assert.equal(calls.length, 1);
  await model.create('修改后的名称', true);
  assert.equal(model.phase.value, 'complete');
  assert.deepEqual(
    calls.map(({ kind }) => kind),
    ['create', 'create'],
  );
});
