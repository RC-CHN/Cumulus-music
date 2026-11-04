import test from 'node:test';
import assert from 'node:assert/strict';
import { useCommunity } from '../src/composables/useCommunity.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type {
  CommunityCard,
  CommunityClient,
  CommunityDetail,
  CommunityFeedAction,
  CommunityFeedPage,
} from '../src/services/community.ts';

const card = (key: string, authorName = key, detail: string | null = key): CommunityCard => ({
  key,
  authorName,
  detail,
  avatarUrl: null,
  eventTime: null,
  timeText: null,
  title: null,
  message: null,
  pictures: [],
  attachment: null,
  status: 'ready',
  counters: { comments: null, likes: null, shares: null },
});
const unreachable = () => {
  throw new Error('unexpected fixture operation');
};

test('first errors differ from empty; retry reuses the failed cursor and updates redacted duplicates', async () => {
  const calls: CommunityFeedAction[] = [];
  let attempts = 0;
  const client: CommunityClient = {
    async communityFeed(epoch, action) {
      calls.push(action);
      if (++attempts === 1 || attempts === 3) throw { code: 'offline', message: '测试断线' };
      if (action.kind === 'refresh')
        return { epoch, value: { cards: [], continuation: null, replace: true } };
      if (action.kind === 'next')
        return {
          epoch,
          value: {
            cards: [card('first', '匿名用户', null), card('next')],
            continuation: null,
            replace: false,
          },
        };
      return {
        epoch,
        value: { cards: [card('first', '原作者')], continuation: 'ticket', replace: true },
      };
    },
    communityDetail: unreachable,
    communityComments: unreachable,
  };
  const model = useCommunity(client, unreachable);
  model.connect('1');
  await model.ensure();
  assert.equal(model.loaded.value, false);
  assert.equal(model.feedError.value, '测试断线');
  await model.retry();
  assert.equal(model.loaded.value, true);
  assert.equal(model.visible.value[0]?.authorName, '原作者');
  await model.next();
  assert.equal(model.feedError.value, '测试断线');
  assert.equal(model.visible.value.length, 1);
  await model.retry();
  assert.deepEqual(
    model.visible.value.map((item) => item.authorName),
    ['匿名用户', 'next'],
  );
  assert.equal(model.visible.value[0]?.detail, null);
  await model.fresh();
  assert.equal(model.loaded.value, true);
  assert.equal(model.visible.value.length, 0);
  assert.deepEqual(calls, [
    { kind: 'first' },
    { kind: 'first' },
    { kind: 'next', continuation: 'ticket' },
    { kind: 'next', continuation: 'ticket' },
    { kind: 'refresh' },
  ]);
});

test('switching account or detail suppresses late private results and their dependent comment reads', async () => {
  let oldFeed!: (value: AccountData<CommunityFeedPage>) => void;
  let oldDetail!: (value: AccountData<CommunityDetail | null>) => void;
  const detailReads: string[] = [];
  const model = useCommunity(
    {
      communityFeed(epoch) {
        if (epoch === 'old') return new Promise((resolve) => (oldFeed = resolve));
        return Promise.resolve({
          epoch,
          value: { cards: [card('new')], continuation: null, replace: true },
        });
      },
      communityDetail(epoch, key) {
        detailReads.push(key);
        if (epoch === 'old') return new Promise((resolve) => (oldDetail = resolve));
        return Promise.resolve({ epoch, value: null });
      },
      communityComments: unreachable,
    },
    unreachable,
  );
  model.connect('old');
  const pendingFeed = model.ensure();
  const pendingDetail = model.openDetail('old-handle');
  model.connect('new');
  await model.ensure();
  await model.openDetail('new-handle');
  oldFeed({
    epoch: 'old',
    value: { cards: [card('old')], continuation: 'old-cursor', replace: true },
  });
  oldDetail({ epoch: 'old', value: { card: card('old'), commentsAvailable: true } });
  await Promise.all([pendingFeed, pendingDetail]);
  assert.deepEqual(
    model.visible.value.map((item) => item.key),
    ['new'],
  );
  assert.equal(model.detail.value, null);
  assert.equal(model.handle.value, 'new-handle');
  assert.deepEqual(detailReads, ['old-handle', 'new-handle']);
});

test('a late anonymous update revokes a detail already opened from the prior feed', async () => {
  let continuation!: (value: AccountData<CommunityFeedPage>) => void;
  const model = useCommunity(
    {
      communityFeed(epoch, action) {
        if (action.kind === 'next') return new Promise((resolve) => (continuation = resolve));
        return Promise.resolve({
          epoch,
          value: { cards: [card('shared', '原作者')], continuation: 'cursor', replace: true },
        });
      },
      async communityDetail(epoch) {
        return { epoch, value: { card: card('shared', '原作者'), commentsAvailable: false } };
      },
      communityComments: unreachable,
    },
    unreachable,
  );
  model.connect('1');
  await model.ensure();
  const pending = model.next();
  await model.openDetail('shared');
  assert.equal(model.detail.value?.card.authorName, '原作者');
  continuation({
    epoch: '1',
    value: { cards: [card('shared', '匿名用户', null)], continuation: null, replace: false },
  });
  await pending;
  assert.equal(model.detail.value, null);
  assert.equal(model.handle.value, 'shared');
  assert.equal(model.visible.value[0]?.authorName, '匿名用户');
});
