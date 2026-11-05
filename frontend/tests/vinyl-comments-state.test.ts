import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref } from 'vue';
import { useVinylComments } from '../src/composables/useVinylComments.ts';
import { usePlaybackCommentInfo } from '../src/composables/usePlaybackCommentCount.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type { VinylCommentPage } from '../src/services/comments.ts';
import type { PlayerView } from '../src/services/player.ts';

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
