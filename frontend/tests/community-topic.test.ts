import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useCommunityTopic } from '../src/composables/useCommunityTopic.ts';
import { topicParts } from '../src/services/communityTopic.ts';
import type { CommunityTopicClient } from '../src/services/communityTopic.ts';

test('complete literal note hashtags become internal labels while unsafe text stays text', () => {
  const input = '<b>#听歌 100%#</b> 与 #第二个话题#，非法 #坏/路径#';
  const parts = topicParts(input);
  assert.equal(parts.map((part) => part.text).join(''), input);
  assert.deepEqual(
    parts.flatMap((part) => (part.topic ? [part.topic] : [])),
    ['听歌 100%', '第二个话题'],
  );
  assert.equal(
    topicParts(Array.from({ length: 29 }, (_, index) => `#标签${index}#`).join(' ')).filter(
      (part) => part.topic,
    ).length,
    24,
  );
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}

async function topicVisit(overrides: Partial<CommunityTopicClient>) {
  const client: CommunityTopicClient = {
    async communityTopic(epoch) {
      return {
        epoch,
        value: {
          handle: 'topic-ticket',
          title: '合成话题',
          description: null,
          coverUrl: null,
          wideCoverUrl: null,
          participants: null,
        },
      };
    },
    async communityTopicSlice(epoch) {
      return {
        epoch,
        value: { cards: [], continuation: null, replace: true, unavailable: false, retired: [] },
      };
    },
    async communityTopicNotice(epoch) {
      return { epoch, value: true };
    },
  };
  const model = useCommunityTopic({ ...client, ...overrides }, assert.fail);
  const target = { name: '合成话题', visit: 1 };
  model.enter('epoch', target);
  await new Promise<void>((resolve) => setImmediate(resolve));
  const state = model.active.value!;
  assert.ok(state.all.loaded && state.hot.loaded);
  return { model, state, target };
}

test('leaving after the first refresh request preserves the existing notice and suppresses Hot', async () => {
  const all = deferred<void>();
  const calls: string[] = [];
  const { model, state, target } = await topicVisit({
    async communityTopicSlice(epoch, _, { kind }) {
      calls.push(kind);
      if (kind === 'refresh') await all.promise;
      return {
        epoch,
        value: { cards: [], continuation: null, replace: true, unavailable: false, retired: [] },
      };
    },
  });
  await model.notice(state, () => true);
  const refresh = model.refresh();
  model.enter('epoch', null);
  model.enter('epoch', target);
  all.resolve();
  await refresh;
  assert.equal(state.hasNew, true);
  assert.deepEqual(calls, ['hot', 'first', 'refresh']);
  await model.refresh();
  assert.equal(state.hasNew, false);
  assert.deepEqual(calls, ['hot', 'first', 'refresh', 'refresh', 'hot']);
});

test('an older foreground notice cannot restore the banner after both refresh reads succeed', async () => {
  const late = deferred<boolean>();
  let notices = 0;
  const { model, state } = await topicVisit({
    async communityTopicNotice(epoch) {
      return { epoch, value: ++notices === 1 ? true : await late.promise };
    },
  });
  await model.notice(state, () => true);
  assert.equal(state.hasNew, true);
  const pendingNotice = model.notice(state, () => true);
  await model.refresh();
  assert.equal(state.hasNew, false);
  late.resolve(true);
  await pendingNotice;
  assert.equal(state.hasNew, false);
});
