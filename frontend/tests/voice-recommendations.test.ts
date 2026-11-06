import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, shallowRef } from 'vue';
import { useVoiceRecommendations } from '../src/composables/useVoiceRecommendations.ts';
import type { PlayerView } from '../src/services/player.ts';
import {
  voiceRecommendationsSeed,
  type VoiceRecommendationsPage,
  type VoiceRecommendationsSeed,
} from '../src/services/voiceRecommendations.ts';

const a = { epoch: '1', playId: 'play-a', voiceId: '601' };
const b = { epoch: '1', playId: 'play-b', voiceId: '602' };

function pending<T>() {
  const { promise, resolve, reject } = Promise.withResolvers<T>();
  return { promise, resolve, reject };
}

function page(seed: VoiceRecommendationsSeed, number = 1, ids = ['91']): VoiceRecommendationsPage {
  return {
    playId: seed.playId,
    voiceId: seed.voiceId,
    receipt: `receipt:${seed.playId}`,
    page: number,
    pageCount: 2,
    cards: ids.map((radioId) => ({ radioId, name: `播单 ${radioId}`, playCount: 0 })),
  };
}

function setup(initial?: VoiceRecommendationsSeed) {
  const scope = effectScope();
  const seed = shallowRef(initial);
  const reads: {
    args: string[];
    response: ReturnType<typeof pending<VoiceRecommendationsPage>>;
  }[] = [];
  const pages: typeof reads = [];
  const enqueue = (list: typeof reads, args: string[]) => {
    const response = pending<VoiceRecommendationsPage>();
    list.push({ args, response });
    return response.promise;
  };
  const model = scope.run(() =>
    useVoiceRecommendations(
      {
        playerVinylVoiceRecommendations: (...args) => enqueue(reads, args),
        playerVinylVoiceRecommendationsNext: (...args) => enqueue(pages, args),
      },
      seed,
    ),
  )!;
  return { model, seed, reads, pages, close: () => scope.stop() };
}

test('only a matching current episode can seed recommendations', () => {
  const view = {
    epoch: '1',
    playId: 'play-a',
    state: 'paused',
    resourceKind: 'downloaded',
    track: { id: '1601', voiceId: '601' },
    voiceDetail: { id: '601', creator: null, releaseTime: null },
  } as PlayerView;
  assert.deepEqual(voiceRecommendationsSeed(view, '1'), a);
  assert.equal(voiceRecommendationsSeed(view, '2'), undefined);
  assert.equal(voiceRecommendationsSeed({ ...view, state: 'loading' }, '1'), undefined);
  assert.equal(voiceRecommendationsSeed({ ...view, voiceDetail: undefined }, '1'), undefined);
});

test('rapid episode switches and closing the panel discard late replies and retire old cards', async () => {
  const { model, seed, reads, close } = setup(a);
  try {
    seed.value = { ...a }; // Clock update, same authority.
    await nextTick();
    assert.equal(reads.length, 1);
    seed.value = b;
    await nextTick();
    assert.deepEqual(
      reads.map(({ args }) => args),
      [
        ['1', 'play-a'],
        ['1', 'play-b'],
      ],
    );
    const current = page(b);
    reads[1]!.response.resolve(current);
    reads[0]!.response.resolve(page(a));
    await nextTick();
    assert.equal(model.view.value, current);
    const target = model.target(current, '91')!;
    assert.equal(model.ownsTarget(target), true);
    assert.equal(model.target(current, '999'), undefined);
    seed.value = undefined;
    seed.value = b; // Same-tick close/reopen is a new panel visit.
    assert.equal(model.ownsTarget(target), false);
    await nextTick();
    assert.equal(reads.length, 3);
    reads[2]!.response.resolve(page(a));
    await nextTick();
    assert.equal(model.view.value, undefined);
  } finally {
    close();
  }
});

test('local page calls are serialized and a delayed previous-account page cannot replace the new shelf', async () => {
  const { model, seed, reads, pages, close } = setup(a);
  try {
    const first = page(a);
    reads[0]!.response.resolve(first);
    await nextTick();
    const target = model.target(first, '91')!;
    const update = model.next();
    await model.next();
    assert.deepEqual(
      pages.map(({ args }) => args),
      [['1', 'play-a', first.receipt]],
    );
    const second = page(a, 2, ['97']);
    pages[0]!.response.resolve(second);
    await update;
    assert.equal(model.view.value, second);
    assert.equal(model.ownsTarget(target), false);
    assert.equal(reads.length, 1);
    const failed = model.next();
    pages[1]!.response.reject(new Error('Native receipt unavailable'));
    await failed;
    assert.equal(model.view.value, second);
    const old = model.next();
    const anotherAccount = { ...a, epoch: '2' };
    seed.value = anotherAccount;
    await nextTick();
    const fresh = { ...first, receipt: 'new-account' };
    reads[1]!.response.resolve(fresh);
    await nextTick();
    pages[2]!.response.resolve(first);
    await old;
    assert.equal(model.view.value, fresh);
    assert.equal(model.paging.value, false);
  } finally {
    close();
  }
});
