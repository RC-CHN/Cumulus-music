import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import {
  offerVoiceSongGroupDownload,
  useVoiceSongGroupDownload,
} from '../src/composables/useVoiceSongGroupDownload.ts';
import type {
  VoiceSongGroupDownloadClient,
  VoiceSongGroupDownloadConfirmation,
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
    playerVoiceSongsDownloadDestination: async () => assert.fail('unexpected destination'),
    playerVoiceSongsDownloadStart: async () => assert.fail('unexpected start'),
    playerVoiceSongsDownloadConfirmQuality: async () => assert.fail('unexpected confirmation'),
    openVoiceSongGroupPurchasedQualityGuide: async () => assert.fail('unexpected guide'),
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

test('the group menu allows either signed anchor but not anonymous or in-flight likes', () => {
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
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, true, new Set()), true);
  assert.equal(offerVoiceSongGroupDownload(card, 0, frozen, true, new Set()), true);
  assert.equal(offerVoiceSongGroupDownload(card, 0, frozen, false, new Set()), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, true, new Set(['1790'])), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, [1, 2, 3], true, new Set()), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, [0, 1], true, new Set()), false);
  assert.equal(offerVoiceSongGroupDownload(card, 2, [2, 3], true, new Set()), false);
  card.availability[0]!.groupDownload = 'purchased';
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, true, new Set()), true);
  card.availability[1]!.groupDownload = 'purchased';
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, true, new Set()), true);
  card.availability[0] = { onlineEligible: true, copyrightBlocked: false };
  assert.equal(offerVoiceSongGroupDownload(card, 2, frozen, true, new Set()), false);
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
  let decision: 'directory_required' | 'membership_unknown' | 'unsupported_selection' =
    'directory_required';
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
    for (const blocked of ['membership_unknown', 'unsupported_selection'] as const) {
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

const purchased = {
  ...plan,
  decision: 'purchased_quality_required',
  purchasedQualities: ['sky', 'hires', 'standard'],
} satisfies VoiceSongGroupDownloadPlan;

test('a purchased group opens without starting, guides the exact chooser, and only confirms once', async () => {
  const calls: unknown[] = [];
  let finish!: (value: VoiceSongGroupDownloadConfirmation) => void;
  const accepted = {
    epoch: owner.epoch,
    accepted: ['one', 'two'],
    failed: [],
    operationId: selectionReceipt,
  };
  const secondReceipt = 'next-native-selection' as VoiceSongGroupDownloadReceipt;
  let prepares = 0;
  const { scope, model } = setup(
    client({
      playerVoiceSongsDownloadPrepare: async (request) => {
        calls.push(['prepare', request]);
        return prepares++ ? { ...purchased, selectionReceipt: secondReceipt } : purchased;
      },
      playerVoiceSongsDownloadCancel: async (target) => {
        calls.push(['cancel', target]);
      },
      openVoiceSongGroupPurchasedQualityGuide: async (target) => {
        calls.push(['guide', target]);
      },
      playerVoiceSongsDownloadConfirmQuality: (target, tier) => {
        calls.push(['confirm', target, tier]);
        return new Promise((resolve) => (finish = resolve));
      },
    }),
  );
  try {
    await model.run(opening);
    assert.deepEqual(model.quality.value?.plan.purchasedQualities, ['sky', 'hires', 'standard']);
    assert.deepEqual(calls, [['prepare', opening.request]]);
    await model.openQualityGuide();
    await model.confirmQuality('vivid');
    model.closeQuality();
    assert.equal(model.quality.value, undefined);
    await model.confirmQuality('hires');
    assert.deepEqual(calls, [
      ['prepare', opening.request],
      ['guide', target],
      ['cancel', target],
    ]);

    await model.run(opening);
    const choice = model.confirmQuality('hires');
    assert.equal(model.busy.value, true);
    model.closeQuality();
    await model.confirmQuality('sky');
    assert.deepEqual(calls.slice(3), [
      ['prepare', opening.request],
      ['confirm', { ...target, selectionReceipt: secondReceipt }, 'hires'],
    ]);
    finish({ kind: 'started', result: { ...accepted, operationId: secondReceipt } });
    await choice;
    assert.equal(model.quality.value, undefined);
    assert.equal(model.notice.value?.kind, 'queued');
    assert.equal(calls.length, 5);
  } finally {
    scope.stop();
  }
});

test('known blocked confirmations retire only the chooser and never retry after the settings link', async () => {
  const calls: unknown[] = [];
  const navigation: string[] = [];
  let reply: VoiceSongGroupDownloadConfirmation = { kind: 'directory_required' };
  const { scope, model } = setup(
    client({
      playerVoiceSongsDownloadPrepare: async () => purchased,
      playerVoiceSongsDownloadConfirmQuality: async (...args) => {
        calls.push(['confirm', ...args]);
        return reply;
      },
      playerVoiceSongsDownloadCancel: async (target) => {
        calls.push(['cancel', target]);
      },
    }),
    navigation,
  );
  try {
    await model.run(opening);
    await model.confirmQuality('hires');
    assert.equal(model.quality.value, undefined);
    assert.equal(model.notice.value?.kind, 'directory');
    assert.deepEqual(navigation, []);
    model.action();
    assert.deepEqual(navigation, ['settings:4']);
    assert.deepEqual(calls, [
      ['confirm', target, 'hires'],
      ['cancel', target],
    ]);

    for (const kind of ['membership_unknown'] as const) {
      reply = { kind };
      await model.run(opening);
      await model.confirmQuality('standard');
      assert.equal(model.notice.value?.kind, 'blocked');
      model.action();
    }
    assert.deepEqual(calls.slice(2), [
      ['confirm', target, 'standard'],
      ['cancel', target],
    ]);
    assert.deepEqual(navigation, ['settings:4']);
  } finally {
    scope.stop();
  }
});

test('retiring a purchased visit cancels only prepared work and never retries a submitted or lost reply', async () => {
  const live = ref(true);
  const calls: unknown[] = [];
  const navigation: string[] = [];
  let finish!: (value: VoiceSongGroupDownloadConfirmation) => void;
  let drop = false;
  const { scope, model } = setup(
    client({
      playerVoiceSongsDownloadPrepare: async () => purchased,
      playerVoiceSongsDownloadConfirmQuality: async (...args) => {
        calls.push(['confirm', ...args]);
        if (drop) throw new Error('synthetic lost confirmation');
        return new Promise((resolve) => (finish = resolve));
      },
      playerVoiceSongsDownloadCancel: async (target) => {
        calls.push(['cancel', target]);
      },
    }),
    navigation,
  );
  try {
    await model.run({ ...opening, live: () => live.value });
    live.value = false;
    assert.deepEqual(calls, [['cancel', target]]);
    assert.equal(model.quality.value, undefined);
    live.value = true;
    await model.run({ ...opening, live: () => live.value });
    const pending = model.confirmQuality('sky');
    live.value = false;
    finish({
      kind: 'started',
      result: {
        epoch: owner.epoch,
        accepted: ['one', 'two'],
        failed: [],
        operationId: selectionReceipt,
      },
    });
    await pending;
    assert.equal(model.notice.value, undefined);
    assert.deepEqual(calls, [
      ['cancel', target],
      ['confirm', target, 'sky'],
    ]);

    drop = true;
    await model.run(opening);
    await model.confirmQuality('hires');
    assert.equal(model.quality.value, undefined);
    assert.equal(model.notice.value?.kind, 'uncertain');
    model.action();
    assert.deepEqual(navigation, ['downloads:4']);
    assert.deepEqual(calls.slice(2), [['confirm', target, 'hires']]);
  } finally {
    scope.stop();
  }
});

test('ordinary picker is read-only before an explicit target; unknown add is never repeated', async () => {
  const calls: string[] = [];
  const playlist = {
    epoch: owner.epoch,
    playId: owner.playId,
    selectionReceipt,
    playlistId: '81',
    operationId: 'add',
    phase: 'uncertain',
    addFailed: false,
    continuation: null,
  } as const;
  const { scope, model } = setup(
    client({
      playerVoiceSongsDownloadPrepare: async () => ({ ...plan, decision: 'playlist_required' }),
      playerVoiceSongsDownloadDestination: async (got, command) => {
        assert.deepEqual(got, target);
        calls.push(command.kind);
        if (command.kind === 'liked_destination')
          return {
            kind: 'liked_destination',
            destination: {
              epoch: owner.epoch,
              value: { playlistId: '81', destinationReceipt: 'star' },
            },
          };
        if (command.kind === 'choose_liked') {
          assert.equal(command.destinationReceipt, 'star');
          return { kind: 'playlist', playlist };
        }
        if (command.kind === 'status') {
          assert.equal(command.reconcile, true);
          return {
            kind: 'playlist',
            playlist: { ...playlist, phase: 'confirmed', continuation: 'journal' },
          };
        }
        if (command.kind === 'start') {
          assert.equal(command.continuation, 'journal');
          return {
            kind: 'started',
            result: {
              epoch: owner.epoch,
              accepted: ['t1', 't2'],
              failed: [],
              operationId: selectionReceipt,
            },
          };
        }
        return assert.fail('unexpected command');
      },
    }),
  );
  try {
    await model.run(opening);
    assert.equal(model.destination.visible.value, true);
    assert.deepEqual(calls, ['liked_destination']);
    await new Promise<void>((done) => setImmediate(done));
    assert.equal(model.destination.liked.value?.playlistId, '81');
    await model.destination.chooseLiked();
    assert.equal(model.destination.phase.value, 'pending');
    model.destination.close();
    assert.equal(model.destination.visible.value, false);
    assert.equal(model.notice.value?.kind, 'playlist_pending');
    model.action();
    assert.equal(model.destination.visible.value, true);
    await model.destination.reconcile();
    assert.deepEqual(calls, ['liked_destination', 'choose_liked', 'status', 'start']);
    assert.equal(model.notice.value?.kind, 'queued');
    assert.equal(model.destination.visible.value, false);
  } finally {
    scope.stop();
  }
});

test('purchased tier precedes any destination reads and a stale account cannot continue a sent write', async () => {
  const live = ref(true);
  const calls: string[] = [];
  let answer!: (
    reply: import('../src/services/voiceSongs.ts').VoiceSongGroupDestinationReply,
  ) => void;
  const { scope, model } = setup(
    client({
      playerVoiceSongsDownloadPrepare: async () => purchased,
      playerVoiceSongsDownloadConfirmQuality: async (_, tier) => {
        calls.push(tier);
        return { kind: 'playlist_required' };
      },
      playerVoiceSongsDownloadCancel: async () => {
        calls.push('cancel');
      },
      playerVoiceSongsDownloadDestination: async (_, command) => {
        calls.push(command.kind);
        if (command.kind === 'liked_destination')
          return { kind: 'liked_destination', destination: { epoch: owner.epoch, value: null } };
        if (command.kind === 'choose')
          return new Promise((resolve) => {
            answer = resolve;
          });
        return assert.fail('unexpected command');
      },
    }),
  );
  try {
    await model.run({ ...opening, live: () => live.value });
    assert.deepEqual(calls, []);
    assert.equal(model.destination.visible.value, false);
    await model.confirmQuality('hires');
    assert.deepEqual(calls, ['hires', 'liked_destination']);
    assert.equal(model.quality.value, undefined);
    const write = model.destination.choose('82');
    await new Promise<void>((done) => setImmediate(done));
    assert.deepEqual(calls, ['hires', 'liked_destination', 'choose']);
    live.value = false;
    answer({
      kind: 'playlist',
      playlist: {
        epoch: owner.epoch,
        playId: owner.playId,
        selectionReceipt,
        playlistId: '82',
        operationId: 'op',
        phase: 'confirmed',
        addFailed: false,
        continuation: 'journal',
      },
    });
    await write;
    await model.destination.drained();
    assert.deepEqual(calls, ['hires', 'liked_destination', 'choose', 'cancel']);
    assert.equal(model.destination.visible.value, false);
    assert.equal(model.notice.value, undefined);
  } finally {
    scope.stop();
  }
});
