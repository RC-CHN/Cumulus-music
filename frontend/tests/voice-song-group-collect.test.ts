import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useVoiceSongGroupCollect } from '../src/composables/useVoiceSongGroupCollect.ts';
import type {
  VoiceSongCollectClient,
  VoiceSongCollectCreation,
  VoiceSongCollectCreationCommand,
  VoiceSongCollectPlan,
  VoiceSongCollectResult,
  VoiceSongLikedCollectClient,
} from '../src/services/voiceSongs.ts';

const card = { epoch: '1', playId: 'play', voiceId: '601', cardReceipt: 'card' };
const target = { ...card, selectionReceipt: 'group' };
const plan: VoiceSongCollectPlan = {
  epoch: '1',
  playId: 'play',
  selectionReceipt: 'group',
  selectedCount: 3,
  eligibleCount: 2,
  omittedIndices: [2],
  usedFirstFallback: false,
  canCreate: true,
};
const confirmed: VoiceSongCollectResult = {
  epoch: '1',
  playId: 'play',
  selectionReceipt: 'group',
  playlistId: '81',
  operationId: 'add',
  phase: 'confirmed',
};
const withoutLiked: VoiceSongLikedCollectClient = {
  async playerVoiceSongsCollectLikedDestination() {
    return { epoch: card.epoch, value: null };
  },
  async playerVoiceSongsCollectLiked() {
    return assert.fail('no verified system liked destination');
  },
};
const noCreate: Pick<VoiceSongCollectClient, 'playerVoiceSongsCollectCreateAction'> = {
  async playerVoiceSongsCollectCreateAction() {
    return assert.fail('existing destination never enters new playlist actions');
  },
};

test('a lost add acknowledgement stays bound after closing and explicit reconciliation does not replay it', async () => {
  const scope = effectScope();
  const calls: string[] = [];
  let completed = 0;
  const client: VoiceSongCollectClient & VoiceSongLikedCollectClient = {
    ...withoutLiked,
    ...noCreate,
    async playerVoiceSongsCollectPrepare(value, indices) {
      calls.push('prepare');
      assert.deepEqual(value, card);
      assert.deepEqual(indices, [1, 2, 0]);
      return plan;
    },
    async playerVoiceSongsCollectDestinations(value, sort, page) {
      calls.push('destinations');
      assert.deepEqual(value, target);
      assert.equal(sort, 'recently');
      assert.equal(page, 0);
      return { epoch: '1', value: { playlists: [], nextPage: null } };
    },
    async playerVoiceSongsCollectExisting(value, id) {
      calls.push('add');
      assert.deepEqual(value, target);
      assert.equal(id, '81');
      throw Error('reply lost');
    },
    async playerVoiceSongsCollectStatus(value, reconcile) {
      calls.push(reconcile ? 'reconcile' : 'read journal');
      assert.deepEqual(value, target);
      return reconcile ? confirmed : { ...confirmed, phase: 'uncertain' };
    },
  };
  const model = scope.run(() => useVoiceSongGroupCollect(client, assert.fail, () => completed++))!;
  try {
    const origin = { card, indices: [1, 2, 0], live: () => true };
    await model.open(origin);
    await model.destinations('recently', 0);
    await model.choose('81');
    assert.equal(model.phase.value, 'pending');
    model.close();
    assert.equal(model.visible.value, false);
    await model.open({ ...origin, indices: [3, 0] });
    assert.equal(model.visible.value, true);
    await model.choose('82');
    assert.equal(model.destinations('default', 0), undefined);
    await model.reconcile();
    assert.equal(completed, 1);
    assert.equal(model.visible.value, false);
    assert.deepEqual(calls, ['prepare', 'destinations', 'add', 'read journal', 'reconcile']);
  } finally {
    scope.stop();
  }
});

test('an asynchronously prepared selection cannot reappear after its playback visit retires', async () => {
  const scope = effectScope();
  const live = ref(true);
  let resolve!: (result: VoiceSongCollectPlan) => void;
  const client: VoiceSongCollectClient & VoiceSongLikedCollectClient = {
    ...withoutLiked,
    ...noCreate,
    playerVoiceSongsCollectPrepare: () => new Promise((done) => (resolve = done)),
    async playerVoiceSongsCollectDestinations() {
      return assert.fail();
    },
    async playerVoiceSongsCollectExisting() {
      return assert.fail();
    },
    async playerVoiceSongsCollectStatus() {
      return assert.fail();
    },
  };
  const model = scope.run(() => useVoiceSongGroupCollect(client, assert.fail, assert.fail))!;
  try {
    const opening = model.open({ card, indices: [1, 2, 0], live: () => live.value });
    assert.equal(model.visible.value, true);
    live.value = false;
    live.value = true;
    resolve(plan);
    await opening;
    assert.equal(model.visible.value, false);
    assert.equal(model.plan.value, undefined);
    assert.equal(model.destinations('default', 0), undefined);
  } finally {
    scope.stop();
  }
});

test('the system liked row is revoked with the card and submits only its current native receipt', async () => {
  const scope = effectScope();
  const live = ref(true);
  const old = Promise.withResolvers<{
    epoch: string;
    value: { playlistId: string; destinationReceipt: string };
  }>();
  const actions: string[] = [];
  const liked = { playlistId: '82', destinationReceipt: 'fresh-destination' };
  let reads = 0;
  const client: VoiceSongCollectClient & VoiceSongLikedCollectClient = {
    ...noCreate,
    async playerVoiceSongsCollectPrepare() {
      return plan;
    },
    async playerVoiceSongsCollectDestinations() {
      return { epoch: '1', value: { playlists: [], nextPage: null } };
    },
    async playerVoiceSongsCollectExisting() {
      return assert.fail('must not use ordinary add');
    },
    async playerVoiceSongsCollectLikedDestination(value) {
      assert.deepEqual(value, target);
      actions.push('read liked');
      return ++reads === 1 ? old.promise : { epoch: '1', value: liked };
    },
    async playerVoiceSongsCollectLiked(value, receipt) {
      assert.deepEqual(value, target);
      assert.equal(receipt, liked.destinationReceipt);
      actions.push('add liked');
      return { ...confirmed, playlistId: '82', phase: 'uncertain' };
    },
    async playerVoiceSongsCollectStatus(value, reconcile) {
      assert.deepEqual(value, target);
      assert.equal(reconcile, true);
      actions.push('check liked');
      return { ...confirmed, playlistId: '82' };
    },
  };
  const model = scope.run(() =>
    useVoiceSongGroupCollect(client, assert.fail, () => actions.push('saved')),
  )!;
  try {
    const origin = { card, indices: [1, 2, 0], live: () => live.value };
    await model.open(origin);
    await model.chooseLiked();
    assert.deepEqual(actions, ['read liked']);
    live.value = false;
    old.resolve({ epoch: '1', value: { playlistId: '93', destinationReceipt: 'retired' } });
    await Promise.resolve();
    assert.equal(model.liked.value, undefined);
    live.value = true;
    await model.open(origin);
    assert.deepEqual(model.liked.value, liked);
    await model.chooseLiked();
    assert.equal(model.phase.value, 'pending');
    await model.chooseLiked();
    await model.reconcile();
    assert.deepEqual(actions, ['read liked', 'read liked', 'add liked', 'check liked', 'saved']);
  } finally {
    scope.stop();
  }
});

function createClient(
  create: VoiceSongCollectClient['playerVoiceSongsCollectCreateAction'],
  canCreate = true,
): VoiceSongCollectClient & VoiceSongLikedCollectClient {
  return {
    ...withoutLiked,
    async playerVoiceSongsCollectPrepare(value, indices) {
      assert.deepEqual(value, card);
      assert.deepEqual(indices, [1, 2, 0]);
      return { ...plan, canCreate };
    },
    async playerVoiceSongsCollectDestinations() {
      return assert.fail('creating does not consult or reselect an existing destination');
    },
    async playerVoiceSongsCollectExisting() {
      return assert.fail('creating cannot call existing playlist mutation');
    },
    async playerVoiceSongsCollectStatus() {
      return assert.fail('creating cannot read the journal of a regular collection');
    },
    playerVoiceSongsCollectCreateAction(value, command) {
      assert.deepEqual(value, target);
      return create(value, command);
    },
  };
}

const origin = { card, indices: [1, 2, 0], live: () => true };
const created: VoiceSongCollectCreation = {
  epoch: '1',
  playId: 'play',
  selectionReceipt: 'group',
  createOperationId: 'create',
  createPhase: 'confirmed',
  playlistId: '81',
  canAdd: true,
  addFailed: false,
  add: null,
};

test('only a verified native group can enter Create; closing the form only retires the prepared receipt', async () => {
  const scope = effectScope();
  const denied = scope.run(() =>
    useVoiceSongGroupCollect(
      createClient(noCreate.playerVoiceSongsCollectCreateAction, false),
      assert.fail,
      assert.fail,
    ),
  )!;
  const calls: VoiceSongCollectCreationCommand[] = [];
  const model = scope.run(() =>
    useVoiceSongGroupCollect(
      createClient(async (_target, command) => {
        calls.push(command);
        return { kind: 'cancelled' };
      }),
      assert.fail,
      assert.fail,
    ),
  )!;
  try {
    await denied.open(origin);
    assert.equal(denied.canCreate.value, false);
    denied.chooseCreate();
    assert.equal(denied.phase.value, 'picking');
    await denied.create('新歌单', true);
    await model.open(origin);
    assert.equal(model.canCreate.value, true);
    model.chooseCreate();
    assert.equal(model.phase.value, 'creating');
    await model.create('无效#名', true);
    await model.create('全'.repeat(21), true);
    assert.deepEqual(calls, []);
    model.close();
    await model.create('新歌单', true);
    assert.equal(model.visible.value, false);
    assert.deepEqual(calls, [{ kind: 'cancel_prepared' }]);
  } finally {
    scope.stop();
  }
});

test('readback of uncertain create is read-only and requires explicit first add for the same receipts', async () => {
  const scope = effectScope();
  const calls: VoiceSongCollectCreationCommand[] = [];
  let completed = 0;
  let reads = 0;
  const model = scope.run(() =>
    useVoiceSongGroupCollect(
      createClient(async (_target, command) => {
        calls.push(command);
        switch (command.kind) {
          case 'create':
            throw { code: 'connection', message: '响应丢失' };
          case 'status':
            if (!command.reconcile)
              return {
                kind: 'creation',
                creation: { ...created, createPhase: 'uncertain', canAdd: false, playlistId: null },
              };
            return {
              kind: 'creation',
              creation: ++reads === 1 ? { ...created, createOperationId: 'foreign' } : created,
            };
          case 'add_created':
            return { kind: 'creation', creation: { ...created, canAdd: false, add: confirmed } };
          default:
            return assert.fail();
        }
      }),
      assert.fail,
      () => completed++,
    ),
  )!;
  try {
    await model.open(origin);
    model.chooseCreate();
    await model.create(' 新歌单 ', true);
    assert.equal(model.phase.value, 'pending');
    model.close();
    await model.reconcile(); // No hidden read after the modal closes.
    await model.open({ ...origin, indices: [0, 1] });
    assert.equal(model.phase.value, 'pending');
    await model.reconcile();
    assert.equal(model.phase.value, 'pending');
    await model.addCreated();
    await model.reconcile();
    assert.equal(model.phase.value, 'created_ready');
    assert.equal(completed, 0);
    await model.create('不同歌单', false);
    await model.addCreated();
    assert.equal(completed, 1);
    assert.deepEqual(calls, [
      { kind: 'create', name: '新歌单', privatePlaylist: true },
      { kind: 'status', reconcile: false },
      { kind: 'status', reconcile: true },
      { kind: 'status', reconcile: true },
      { kind: 'add_created' },
    ]);
  } finally {
    scope.stop();
  }
});

test('a definitive false add does not display success even with confirmed observed membership', async () => {
  const scope = effectScope();
  const calls: VoiceSongCollectCreationCommand[] = [];
  const model = scope.run(() =>
    useVoiceSongGroupCollect(
      createClient(async (_target, command) => {
        calls.push(command);
        return {
          kind: 'creation',
          creation: { ...created, canAdd: false, add: confirmed, addFailed: true },
        };
      }),
      assert.fail,
      assert.fail,
    ),
  )!;
  try {
    await model.open(origin);
    model.chooseCreate();
    await model.create('合成公开歌单', false);
    assert.equal(model.phase.value, 'failed');
    assert.equal(model.message.value, '歌单已创建，但所选歌曲未能收藏。');
    await model.reconcile();
    await model.addCreated();
    assert.deepEqual(calls, [{ kind: 'create', name: '合成公开歌单', privatePlaylist: false }]);
  } finally {
    scope.stop();
  }
});

test('a late creation cannot reopen the dialog or create an action after the active episode retires', async () => {
  const scope = effectScope();
  const live = ref(true);
  const held =
    Promise.withResolvers<
      Awaited<ReturnType<VoiceSongCollectClient['playerVoiceSongsCollectCreateAction']>>
    >();
  const calls: string[] = [];
  const model = scope.run(() =>
    useVoiceSongGroupCollect(
      createClient((_target, command) => {
        calls.push(command.kind);
        return held.promise;
      }),
      assert.fail,
      assert.fail,
    ),
  )!;
  try {
    await model.open({ ...origin, live: () => live.value });
    model.chooseCreate();
    const pending = model.create('新歌单', true);
    live.value = false;
    held.resolve({ kind: 'creation', creation: created });
    await pending;
    await model.addCreated();
    assert.equal(model.visible.value, false);
    assert.deepEqual(calls, ['create']);
  } finally {
    scope.stop();
  }
});
