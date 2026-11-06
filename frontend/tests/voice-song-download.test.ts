import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useVoiceSongDownload } from '../src/composables/useVoiceSongDownload.ts';
import { defaultDownloadOptions } from '../src/services/downloads.ts';
import type {
  VoiceSongDownloadClient,
  VoiceSongDownloadPicker,
  VoiceSongDownloadPlan,
} from '../src/services/voiceSongs.ts';

const target = { epoch: '1', playId: 'play', voiceId: '601', receipt: 'receipt', index: 4 };
const opening = { target, trackId: '1601', title: '合成歌曲' };
const scope = { epoch: target.epoch, playId: target.playId, trackId: opening.trackId };
const added = { epoch: target.epoch, accepted: ['task'], failed: [], operationId: null };
const picker = (value: Partial<VoiceSongDownloadPicker> = {}): VoiceSongDownloadPicker => ({
  ...scope,
  playlistId: '51',
  operationId: 'add',
  phase: 'confirmed',
  addFailed: false,
  continuation: 'opaque',
  ...value,
});

function setup(options: Partial<VoiceSongDownloadClient> = {}) {
  const available = ref(true);
  const calls: string[] = [];
  const implementation: VoiceSongDownloadClient = {
    async playerVoiceSongDownloadPrepare(value) {
      assert.deepEqual(value, target);
      return { ...scope, decision: 'playlist_required' };
    },
    async playerVoiceSongDownloadChoosePlaylist(_target, id) {
      assert.equal(id, '51');
      return picker();
    },
    async playerVoiceSongDownloadPickerStatus(_target, reconcile) {
      assert.equal(reconcile, true);
      return picker();
    },
    async playerVoiceSongDownloadCreatePlaylist(_target, name, privateList) {
      assert.equal(name, '合成歌单');
      assert.equal(privateList, false);
      return {
        ...scope,
        createOperationId: 'create',
        createPhase: 'confirmed',
        playlistId: '51',
        add: null,
      };
    },
    async playerVoiceSongDownloadCreationStatus(_target, reconcile) {
      assert.equal(reconcile, true);
      return {
        ...scope,
        createOperationId: 'create',
        createPhase: 'confirmed',
        playlistId: '51',
        add: null,
      };
    },
    async playerVoiceSongDownloadAddCreatedPlaylist() {
      return {
        ...scope,
        createOperationId: 'create',
        createPhase: 'confirmed',
        playlistId: '51',
        add: picker(),
      };
    },
    async playerVoiceSongDownloadStart() {
      return added;
    },
    async playerVoiceSongDownloadStartFromPlaylist(_target, continuation, value) {
      assert.equal(continuation, 'opaque');
      assert.deepEqual(value, defaultDownloadOptions);
      return added;
    },
    ...options,
  };
  const client = new Proxy(implementation, {
    get(host, key: keyof VoiceSongDownloadClient) {
      return (...args: unknown[]) => {
        calls.push(key.replace('playerVoiceSongDownload', ''));
        return Reflect.apply(host[key], host, args);
      };
    },
  });
  const lifetime = effectScope();
  const model = lifetime.run(() =>
    useVoiceSongDownload(
      client,
      () => available.value,
      () => {
        available.value = false;
      },
    ),
  )!;
  return { available, calls, lifetime, model };
}

test('liked songs bypass the picker; cancellation cannot start an ordinary or picker download', async () => {
  const f = setup({
    async playerVoiceSongDownloadPrepare() {
      return { ...scope, decision: 'ready' };
    },
  });
  try {
    await f.model.request(opening);
    assert.equal(f.model.stage.value.kind, 'options');
    f.model.close();
    assert.equal(await f.model.start(defaultDownloadOptions), false);
    await f.model.request(opening);
    assert.equal(await f.model.start(defaultDownloadOptions), true);
    assert.equal(f.model.stage.value.kind, 'idle');
    assert.deepEqual(f.calls, ['Prepare', 'Prepare', 'Start']);
  } finally {
    f.lifetime.stop();
  }
});

test('existing destination definitive add failure preserves the opaque independent download admission', async () => {
  const f = setup({
    async playerVoiceSongDownloadChoosePlaylist() {
      return picker({ phase: 'rejected', addFailed: true });
    },
  });
  try {
    await f.model.request(opening);
    f.model.openCreate();
    f.model.close();
    await f.model.request(opening);
    await f.model.selectPlaylist('51');
    assert.equal(f.model.stage.value.kind, 'options');
    assert.match(f.model.message.value, /加入歌单失败/);
    assert.equal(await f.model.start(defaultDownloadOptions), true);
    assert.deepEqual(f.calls, ['Prepare', 'Prepare', 'ChoosePlaylist', 'StartFromPlaylist']);
  } finally {
    f.lifetime.stop();
  }
});

test('lost create reply only reads back and requires an explicit add; new add failure cannot start', async () => {
  const f = setup({
    async playerVoiceSongDownloadCreatePlaylist(_target, name, privateList) {
      assert.deepEqual([name, privateList], ['合成歌单', false]);
      throw { code: 'transport', message: 'lost synthetic create reply' };
    },
    async playerVoiceSongDownloadAddCreatedPlaylist() {
      return {
        ...scope,
        createOperationId: 'create',
        createPhase: 'confirmed',
        playlistId: '51',
        add: picker({ phase: 'rejected', addFailed: true, continuation: null }),
      };
    },
  });
  try {
    await f.model.request(opening);
    f.model.openCreate();
    await f.model.submitCreate('  合成歌单  ', false);
    await f.model.resumeAdd();
    assert.deepEqual(f.calls, ['Prepare', 'CreatePlaylist']);
    await f.model.reconcile();
    assert.equal(f.model.stage.value.kind, 'recover');
    assert.deepEqual(f.calls, ['Prepare', 'CreatePlaylist', 'CreationStatus']);
    await f.model.resumeAdd();
    assert.equal(f.model.stage.value.kind, 'notice');
    assert.equal(await f.model.start(defaultDownloadOptions), false);
    assert.deepEqual(f.calls, [
      'Prepare',
      'CreatePlaylist',
      'CreationStatus',
      'AddCreatedPlaylist',
    ]);
  } finally {
    f.lifetime.stop();
  }
});

test('closing and reopening the exact same row ignores a late prepare; expiry clears active recovery', async () => {
  const late = Promise.withResolvers<VoiceSongDownloadPlan>();
  let opens = 0;
  const f = setup({
    async playerVoiceSongDownloadPrepare() {
      return ++opens === 1 ? late.promise : { ...scope, decision: 'playlist_required' };
    },
    async playerVoiceSongDownloadChoosePlaylist() {
      throw { code: 'session_expired', message: 'synthetic expired' };
    },
  });
  try {
    const beforeClose = f.model.request(opening);
    f.model.close();
    await f.model.request(opening);
    late.resolve({ ...scope, decision: 'ready' });
    await beforeClose;
    assert.equal(f.model.stage.value.kind, 'pick');
    await f.model.selectPlaylist('51');
    assert.equal(f.available.value, false);
    assert.equal(f.model.stage.value.kind, 'idle');
    assert.equal(f.model.busy.value, false);
  } finally {
    f.lifetime.stop();
  }
});
