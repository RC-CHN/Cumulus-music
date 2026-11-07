import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useVoiceSongGroupCollect } from '../src/composables/useVoiceSongGroupCollect.ts';
import type {
  VoiceSongCollectClient,
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

test('a lost add acknowledgement stays bound after closing and explicit reconciliation does not replay it', async () => {
  const scope = effectScope();
  const calls: string[] = [];
  let completed = 0;
  const client: VoiceSongCollectClient & VoiceSongLikedCollectClient = {
    ...withoutLiked,
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
