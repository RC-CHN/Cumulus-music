import assert from 'node:assert/strict';
import { test } from 'node:test';
import { usePodcasts } from '../src/composables/usePodcasts.ts';
import type { PodcastClient, PodcastQuery } from '../src/services/podcast.ts';

const account = (epoch: string) => ({
  epoch,
  status: 'authenticated' as const,
  nickname: null,
  avatarUrl: null,
});
const radio = (id: string, ascending = true) => ({ kind: 'radio' as const, id, ascending });
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));

function setup() {
  const reads: { epoch: string; query: PodcastQuery; refresh: boolean }[] = [];
  const core: PodcastClient = {
    podcastState: async (epoch) => ({ epoch, value: { likedIds: [], operations: [] } }),
    podcastSubscriptionRecommendations: async (epoch) => ({
      epoch,
      value: { receipt: epoch, cards: [] },
    }),
    podcastRadioOrder: async (epoch) => ({ epoch, value: null }),
    podcastPage: async (epoch, query, refresh) => {
      reads.push({ epoch, query, refresh });
      return {
        epoch,
        scope: query.scope,
        offset: query.offset,
        revision: 'current',
        cards: [],
        episodes: [],
        total: 0,
        remoteTotal: 0,
        radio: null,
        likedIds: [],
        resumeEnabled: null,
        operations: [],
      };
    },
    podcastClearSubscribedBadge: async () => {
      assert.fail('recommendation route must never clear a subscribed badge');
    },
    podcastCommand: async () => {
      assert.fail('recommendation route must never issue a podcast mutation');
    },
  };
  const model = usePodcasts(core);
  model.connect(account('1'));
  model.show(true);
  return { model, reads };
}

test('recommendation Back restores the prior radio query, including offset, keyword and order', async () => {
  const { model, reads } = setup();
  model.navigate(radio('80', false));
  await tick();
  await model.search('上一页筛选');
  await model.paginate(50);
  assert.equal(model.voiceRecommendationReturn.value, false);
  const origin = { ...model.query.value, revision: null };

  model.openVoiceRecommendation('98');
  await tick();
  assert.deepEqual(model.query.value.scope, radio('98'));
  assert.equal(model.voiceRecommendationReturn.value, true);
  const opened = reads.length;
  model.openVoiceRecommendation('98');
  await tick();
  assert.equal(reads.length, opened, 'same radio does not create a new route or read');

  model.openVoiceRecommendation('99');
  await tick();
  model.back();
  await tick();
  assert.deepEqual(model.query.value.scope, radio('98'));
  assert.equal(model.voiceRecommendationReturn.value, true);
  model.back();
  await tick();
  assert.deepEqual(reads.at(-1), { epoch: '1', query: origin, refresh: true });
  assert.deepEqual(model.query.value.scope, radio('80', false));
  assert.equal(model.voiceRecommendationReturn.value, false);
  assert.equal(model.canBack.value, true, 'normal podcast-library history is still available');
  model.back();
  await tick();
  assert.deepEqual(model.query.value.scope, { kind: 'podcasts' });
});

test('recommendation returns to Recent and preserves the former radio’s separate Recent Back', async () => {
  const { model, reads } = setup();
  model.showRecent();
  model.openVoiceRecommendation('98');
  await tick();
  assert.equal(model.recentSelected.value, false);
  assert.equal(model.voiceRecommendationReturn.value, true);
  const opened = reads.length;
  model.back();
  await tick();
  assert.equal(model.recentSelected.value, true);
  assert.equal(model.voiceRecommendationReturn.value, false);
  assert.equal(reads.length, opened, 'restored Recent does not reload a hidden radio');

  model.navigate(radio('80'));
  await tick();
  assert.equal(model.recentReturn.value, true);
  model.openVoiceRecommendation('98');
  await tick();
  assert.equal(model.recentReturn.value, false);
  model.back();
  await tick();
  assert.deepEqual(model.query.value.scope, radio('80'));
  assert.equal(model.recentReturn.value, true);
  assert.equal(model.voiceRecommendationReturn.value, false);
  model.back();
  assert.equal(model.recentSelected.value, true);
});

test('an account switch or unrelated podcast navigation retires contextual Back', async () => {
  const { model, reads } = setup();
  model.navigate(radio('80'), false);
  model.openVoiceRecommendation('98');
  await tick();
  model.navigate(radio('90'));
  await tick();
  assert.equal(model.voiceRecommendationReturn.value, false);
  assert.equal(model.canBack.value, false);
  model.back();
  assert.deepEqual(model.query.value.scope, radio('90'));

  model.openVoiceRecommendation('98');
  assert.equal(model.voiceRecommendationReturn.value, true);
  model.connect(account('2'));
  await tick();
  assert.equal(model.voiceRecommendationReturn.value, false);
  assert.deepEqual(model.query.value.scope, { kind: 'podcasts' });
  model.show(false);
  model.openVoiceRecommendation('98');
  assert.equal(model.voiceRecommendationReturn.value, false);
  assert.ok(
    reads
      .filter((read) => read.epoch === '2')
      .every(({ query }) => query.scope.kind === 'podcasts'),
  );
});
