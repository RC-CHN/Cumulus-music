import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref } from 'vue';
import { useVinylComments } from '../src/composables/useVinylComments.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type { VinylCommentPage } from '../src/services/comments.ts';

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
