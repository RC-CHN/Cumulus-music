import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useVoiceSongGroupCollect } from '../src/composables/useVoiceSongGroupCollect.ts';
import type {
  VoiceSongCollectClient,
  VoiceSongCollectPlan,
  VoiceSongCollectResult,
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

test('a lost add acknowledgement stays bound after closing and explicit reconciliation does not replay it', async () => {
  const scope = effectScope();
  const calls: string[] = [];
  let completed = 0;
  const client: VoiceSongCollectClient = {
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
  const client: VoiceSongCollectClient = {
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
