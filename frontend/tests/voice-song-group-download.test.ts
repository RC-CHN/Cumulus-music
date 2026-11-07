import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import {
  offerVoiceSongGroupDownload,
  useVoiceSongGroupDownload,
} from '../src/composables/useVoiceSongGroupDownload.ts';
import type {
  VoiceSongGroupDownloadClient,
  VoiceSongGroupDownloadPlan,
  VoiceSongGroupDownloadReceipt,
  VoiceSongGroupDownloadTarget,
  VoiceSongsView,
} from '../src/services/voiceSongs.ts';

const owner = { epoch: '4', playId: 'voice-play', cardReceipt: 'native-card' };
const selectionReceipt = 'native-selection' as VoiceSongGroupDownloadReceipt;
const target = { ...owner, selectionReceipt };
const frozen = [1, 0, 2];
const opening = { request: { ...owner, anchorIndex: 2, indices: frozen }, live: () => true };
const plan = {
  epoch: owner.epoch,
  playId: owner.playId,
  anchorIndex: 2,
  eligibleIndices: [1, 0],
  omittedIndices: [2],
  decision: 'ready',
  selectionReceipt,
} satisfies VoiceSongGroupDownloadPlan;

function client(override: Partial<VoiceSongGroupDownloadClient>): VoiceSongGroupDownloadClient {
  return {
    playerVoiceSongsDownloadPrepare: async () => assert.fail('unexpected prepare'),
    playerVoiceSongsDownloadCancel: async () => assert.fail('unexpected cancel'),
    playerVoiceSongsDownloadStart: async () => assert.fail('unexpected start'),
    ...override,
  };
}

function setup(api: VoiceSongGroupDownloadClient, callbacks: string[] = []) {
  const scope = effectScope();
  const model = scope.run(() =>
    useVoiceSongGroupDownload(
      api,
      () => callbacks.push('expired'),
      (epoch) => callbacks.push(`settings:${epoch}`),
      (epoch) => callbacks.push(`downloads:${epoch}`),
    ),
  )!;
  return { scope, model };
}

test('the group menu requires the observed liked trigger and verified ordinary candidate rows', () => {
  const card: VoiceSongsView = {
    playId: owner.playId,
    voiceId: '71',
    receipt: owner.cardReceipt,
    tracks: ['1710', '1712', '1790', '1791'].map((id) => ({
      id,
      name: id,
      artists: [],
      album: '',
      durationMs: 0,
      fee: 0,
      missing: false,
      local: false,
    })),
    availability: [
      { groupDownload: 'ordinary', onlineEligible: true, copyrightBlocked: false },
      { groupDownload: 'ordinary', onlineEligible: true, copyrightBlocked: false },
      { groupDownload: 'omitted', onlineEligible: false, copyrightBlocked: true },
      { groupDownload: 'unsupported', onlineEligible: true, copyrightBlocked: false },
    ],
  };
  const liked = new Set(['1790', '1791']);
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, liked, new Set()), true);
  assert.equal(offerVoiceSongGroupDownload(card, 0, frozen, liked, new Set()), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, liked, new Set(['1790'])), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, [1, 2, 3], liked, new Set()), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, [0, 1], liked, new Set()), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, [2, 3], liked, new Set()), false);
  card.availability[0] = { onlineEligible: true, copyrightBlocked: false };
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, liked, new Set()), false);
});

test('clicking the group action uses frozen ordinals then starts exactly once with only the receipt', async () => {
  let resolve!: (value: VoiceSongGroupDownloadPlan) => void;
  const calls: unknown[] = [];
  const api = client({
    playerVoiceSongsDownloadPrepare: (request) => {
      calls.push(request);
      return new Promise((done) => (resolve = done));
    },
    playerVoiceSongsDownloadStart: async (request) => {
      calls.push(request);
      return {
        epoch: owner.epoch,
        accepted: ['first', 'second'],
        failed: [],
        operationId: selectionReceipt,
      };
    },
  });
  const navigation: string[] = [];
  const { scope, model } = setup(api, navigation);
  try {
    const indices = [...frozen];
    const first = model.run({ ...opening, request: { ...opening.request, indices } });
    assert.equal(model.busy.value, true);
    await model.run(opening);
    indices.reverse();
    resolve(plan);
    await first;
    assert.deepEqual(calls, [opening.request, target]);
    assert.deepEqual(model.notice.value, {
      kind: 'queued',
      text: '已添加到下载列表（2 首，跳过 1 首）',
    });
    model.action();
    assert.deepEqual(navigation, []);
  } finally {
    scope.stop();
  }
});

test('directory and unavailable branches never start; only the explicit settings action navigates', async () => {
  let decision:
    | 'directory_required'
    | 'playlist_required'
    | 'membership_unknown'
    | 'unsupported_selection' = 'directory_required';
  const api = client({
    playerVoiceSongsDownloadPrepare: async () => ({ ...plan, decision, selectionReceipt: null }),
  });
  const calls: string[] = [];
  const { scope, model } = setup(api, calls);
  const live = ref(true);
  try {
    await model.run({ ...opening, live: () => live.value });
    assert.equal(model.notice.value?.kind, 'directory');
    assert.deepEqual(calls, []);
    model.action();
    assert.deepEqual(calls, ['settings:4']);
    live.value = false;
    model.action();
    assert.equal(model.notice.value, undefined);
    assert.deepEqual(calls, ['settings:4']);
    for (const blocked of [
      'playlist_required',
      'membership_unknown',
      'unsupported_selection',
    ] as const) {
      decision = blocked;
      await model.run(opening);
      assert.equal(model.notice.value?.kind, 'blocked');
      model.action();
    }
    assert.deepEqual(calls, ['settings:4']);
  } finally {
    scope.stop();
  }
});

test('retiring the episode during prepare disarms only the exact ready receipt, without a start', async () => {
  let resolve!: (value: VoiceSongGroupDownloadPlan) => void;
  const cancelled: VoiceSongGroupDownloadTarget[] = [];
  const api = client({
    playerVoiceSongsDownloadPrepare: () => new Promise((done) => (resolve = done)),
    playerVoiceSongsDownloadCancel: async (target) => {
      cancelled.push(target);
    },
  });
  const { scope, model } = setup(api);
  const live = ref(true);
  try {
    const pending = model.run({ ...opening, live: () => live.value });
    live.value = false;
    resolve(plan);
    await pending;
    assert.deepEqual(cancelled, [target]);
    assert.equal(model.notice.value, undefined);
    assert.equal(model.busy.value, false);
  } finally {
    scope.stop();
  }
});

test('a mismatched order cannot start; an ambiguous acknowledgement prompts readback without replay', async () => {
  let prepares = 0;
  let starts = 0;
  const navigation: string[] = [];
  const { scope, model } = setup(
    client({
      playerVoiceSongsDownloadPrepare: async () =>
        ++prepares === 1 ? { ...plan, eligibleIndices: [0, 1] } : plan,
      playerVoiceSongsDownloadStart: async () => {
        starts++;
        throw new Error('synthetic lost reply');
      },
    }),
    navigation,
  );
  try {
    await model.run(opening);
    assert.equal(starts, 0);
    assert.equal(model.notice.value?.kind, 'blocked');
    await model.run(opening);
    assert.equal(starts, 1);
    assert.equal(model.notice.value?.kind, 'uncertain');
    assert.deepEqual(navigation, []);
    model.action();
    assert.equal(starts, 1);
    assert.deepEqual(navigation, ['downloads:4']);
  } finally {
    scope.stop();
  }
});
