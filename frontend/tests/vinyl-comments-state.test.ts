import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref } from 'vue';
import { useVinylComments } from '../src/composables/useVinylComments.ts';
import { usePlaybackCommentInfo } from '../src/composables/usePlaybackCommentCount.ts';
import type { AccountData } from '../src/services/contracts.ts';
import {
  vinylLikeCount,
  type CommentItem,
  type CommentOperation,
  type VinylCommentPage,
} from '../src/services/comments.ts';
import type { PlayerView } from '../src/services/player.ts';

const unusedLikes = {
  playerVinylCommentLikeOperations: async (epoch: string) => ({ epoch, value: [] }),
  playerVinylCommentLike: async () => {
    throw new Error('fixture did not enable likes');
  },
};

test('a known zero badge increases once for the exact confirmed play', async () => {
  const scope = effectScope();
  try {
    const epoch = ref('1');
    const player = ref({ epoch: '1', playId: 'play-a', resourceKind: 'catalog' } as PlayerView);
    const loaded = ref(true);
    const info = scope.run(() =>
      usePlaybackCommentInfo(player, loaded, () => epoch.value, {
        playerCommentCount: async () => 0,
      }),
    )!;
    await Promise.resolve();
    const key = JSON.stringify(['1', 'play-a']);
    assert.equal(info.count.value, 0);
    info.notePublished(key, '901');
    info.notePublished(key, '901');
    assert.equal(info.count.value, 1);
    epoch.value = '2';
    player.value = { ...player.value, epoch: '2', playId: 'play-b' };
    info.notePublished(key, '902');
    assert.equal(info.count.value, undefined);
    await nextTick();
    assert.equal(info.count.value, 0);
  } finally {
    scope.stop();
  }
});

test('a confirmed vinyl comment is shown once without advancing the raw server cursor', async () => {
  const scope = effectScope();
  try {
    const key = ref<string | undefined>(JSON.stringify(['1', 'play-a']));
    const count = ref<number | undefined>(61);
    const active = ref(true);
    const cursors: number[] = [];
    const item = (id: string) => ({
      id,
      content: id,
      time: 0,
      timeText: '',
      liked: false,
      likedCount: 0,
      author: null,
      replies: [],
      owned: false,
    });
    const model = scope.run(() =>
      useVinylComments(
        {
          ...unusedLikes,
          playerVinylComments: async (epoch, _playId, offset) => {
            cursors.push(offset);
            return {
              epoch,
              value: {
                latest: offset === 0 ? [item('existing')] : [item('new'), item('older')],
                hot: [],
                moreHot: false,
                nextOffset: offset === 0 ? 60 : null,
              },
            };
          },
        },
        key,
        active,
        count,
      ),
    )!;
    await model.load();
    const oldKey = key.value!;
    assert.equal(model.prependConfirmed(oldKey, item('new')), true);
    assert.equal(model.prependConfirmed(oldKey, item('new')), false);
    await model.load();
    assert.deepEqual(cursors, [0, 60]);
    assert.deepEqual(
      model.latest.value.map((row) => row.id),
      ['new', 'existing', 'older'],
    );
    key.value = JSON.stringify(['1', 'play-b']);
    assert.equal(model.prependConfirmed(oldKey, item('old')), false);
    assert.deepEqual(model.latest.value, []);
  } finally {
    scope.stop();
  }
});

test('vinyl comments defer only visible positive counts and ignore the abandoned read', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const scope = effectScope();
  try {
    const active = ref(false);
    const count = ref<number | undefined>(undefined);
    const key = ref(JSON.stringify(['1', 'play-a']));
    const reads: {
      playId: string;
      resolve: (result: AccountData<VinylCommentPage>) => void;
    }[] = [];
    const model = scope.run(() =>
      useVinylComments(
        {
          ...unusedLikes,
          playerVinylComments: (_epoch, playId) =>
            new Promise((resolve) => reads.push({ playId, resolve })),
        },
        key,
        active,
        count,
      ),
    )!;
    active.value = true;
    count.value = 0;
    await nextTick();
    t.mock.timers.tick(900);
    assert.equal(reads.length, 0);
    count.value = 1;
    await nextTick();
    t.mock.timers.tick(599);
    assert.equal(reads.length, 0);
    t.mock.timers.tick(1);
    assert.equal(reads.length, 1);
    active.value = false;
    await nextTick();
    active.value = true;
    key.value = JSON.stringify(['1', 'play-b']);
    await nextTick();
    t.mock.timers.tick(600);
    assert.equal(reads.length, 2);
    const page = (id: string): AccountData<VinylCommentPage> => ({
      epoch: '1',
      value: {
        latest: [
          {
            id,
            content: id,
            time: 0,
            timeText: '',
            liked: false,
            likedCount: 0,
            author: null,
            replies: [],
            owned: false,
          },
        ],
        hot: [],
        moreHot: false,
        nextOffset: null,
      },
    });
    reads[0]!.resolve(page('old'));
    await Promise.resolve();
    assert.deepEqual(model.latest.value, []);
    assert.equal(model.working.value, true);
    reads[1]!.resolve(page('current'));
    await Promise.resolve();
    assert.equal(model.latest.value[0]?.id, 'current');
    assert.equal(model.working.value, false);
    active.value = false;
    await nextTick();
    active.value = true;
    await nextTick();
    t.mock.timers.tick(600);
    assert.equal(reads.length, 2, 'reopening the same settled thread retains the loaded rows');
  } finally {
    scope.stop();
  }
});

test('vinyl like counts follow the recovered strict 万 and 亿 thresholds', () => {
  assert.deepEqual(
    [0, 1, 100000, 100001, 123499, 100000000, 100000001, 123499999].map(vinylLikeCount),
    ['', '1', '100000', '10万', '12.3万', '10000万', '1亿', '1.2亿'],
  );
});

test('the same verified inline like updates Hot and Latest, fences stale pages and reconciles without replay', async () => {
  const scope = effectScope();
  try {
    const key = ref<string | undefined>(JSON.stringify(['1', 'play-a']));
    const active = ref(true);
    const count = ref<number | undefined>(61);
    const item: CommentItem = {
      id: '21',
      content: '合成评论',
      time: 0,
      timeText: '',
      liked: false,
      likedCount: 3,
      author: null,
      replies: [],
      owned: false,
    };
    const page = (epoch: string, offset: number): AccountData<VinylCommentPage> => ({
      epoch,
      value: {
        latest: offset ? [{ ...item, id: '20' }] : [{ ...item }],
        hot: offset ? [] : [{ ...item }],
        moreHot: false,
        nextOffset: offset ? null : 60,
      },
    });
    let finishAppend: (() => void) | undefined;
    let pending = false;
    let deferredReconcile: (() => void) | undefined;
    const actions: string[] = [];
    const cursors: number[] = [];
    const model = scope.run(() =>
      useVinylComments(
        {
          playerVinylComments: (epoch, _playId, offset) => {
            cursors.push(offset);
            return offset && cursors.length === 2
              ? new Promise((resolve) => {
                  finishAppend = () => resolve(page(epoch, offset));
                })
              : Promise.resolve(page(epoch, offset));
          },
          playerVinylCommentLikeOperations: async (epoch) => ({
            epoch,
            value:
              epoch === '1' && pending
                ? [{ id: item.id, desired: false, phase: 'uncertain', observed: null }]
                : [],
          }),
          playerVinylCommentLike: (epoch, playId, id, action) => {
            assert.deepEqual([epoch, playId, id], ['1', 'play-a', item.id]);
            actions.push(action.kind === 'set' ? String(action.desired) : action.kind);
            if (action.kind === 'set' && !action.desired) {
              pending = true;
              return Promise.reject(new Error('lost synthetic reply'));
            }
            const reply: AccountData<CommentOperation> = {
              epoch,
              value: {
                id,
                desired: action.kind === 'set',
                phase: 'confirmed',
                observed: {
                  ...item,
                  liked: action.kind === 'set',
                  likedCount: action.kind === 'set' ? 4 : 3,
                },
              },
            };
            return action.kind === 'reconcile'
              ? new Promise((resolve) => {
                  deferredReconcile = () => {
                    pending = false;
                    resolve(reply);
                  };
                })
              : Promise.resolve(reply);
          },
        },
        key,
        active,
        count,
      ),
    )!;
    await model.load();
    await nextTick();
    assert.equal(model.likes.ready.value, true);
    const append = model.load();
    const oldItem = model.hot.value[0]!;
    await model.likes.toggle(oldItem);
    finishAppend!();
    await append;
    assert.deepEqual(
      model.latest.value.map(({ id, liked, likedCount }) => [id, liked, likedCount]),
      [['21', true, 4]],
    );
    assert.equal(model.hot.value[0]!.liked, true);
    await model.likes.toggle(oldItem);
    assert.deepEqual(actions, ['true']);
    await model.load();
    assert.deepEqual(cursors, [0, 60, 60]);
    await model.likes.toggle(model.latest.value[0]!);
    assert.equal(model.likes.operations.value.get(item.id)?.phase, 'uncertain');
    assert.equal(model.likes.issue.value?.message, '点赞结果尚未确认，请核对状态。');
    assert.equal(model.latest.value[0]!.liked, true);
    await model.likes.toggle(model.hot.value[0]!);
    assert.deepEqual(actions, ['true', 'false']);
    const verifying = model.likes.reconcile(model.hot.value[0]!);
    key.value = JSON.stringify(['2', 'play-b']);
    await model.load();
    deferredReconcile!();
    await verifying;
    assert.deepEqual(actions, ['true', 'false', 'reconcile']);
    assert.equal(model.likes.operations.value.size, 0);
    assert.equal(model.latest.value[0]!.liked, false);
    assert.equal(model.latest.value[0]!.likedCount, 3);
  } finally {
    scope.stop();
  }
});
