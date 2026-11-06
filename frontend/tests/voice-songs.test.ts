import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, shallowRef } from 'vue';
import { useVoiceSongs } from '../src/composables/useVoiceSongs.ts';
import type { TrackView } from '../src/services/contracts.ts';
import type { VoiceSongsScope, VoiceSongsView } from '../src/services/voiceSongs.ts';

const first = { epoch: '1', playId: 'a', voiceId: '71' };
const second = { epoch: '1', playId: 'b', voiceId: '72' };

function result(scope: VoiceSongsScope, receipt: string | null): VoiceSongsView {
  return {
    playId: scope.playId,
    voiceId: scope.voiceId,
    receipt,
    tracks: [
      { id: '301', name: '第一条' },
      { id: '301', name: '重复条目' },
    ] as TrackView[],
    availability: [
      { onlineEligible: true, copyrightBlocked: false },
      { onlineEligible: false, copyrightBlocked: true },
    ],
  };
}

test('episode songs preserve displayed row positions and revoke late or closed results', async () => {
  const scope = shallowRef<VoiceSongsScope | undefined>(first);
  const calls: {
    args: [string, string];
    resolve: (reply: VoiceSongsView) => void;
  }[] = [];
  const lifetime = effectScope();
  const model = lifetime.run(() =>
    useVoiceSongs(
      {
        playerVoiceSongs(...args) {
          const request = Promise.withResolvers<VoiceSongsView>();
          calls.push({ args, resolve: request.resolve });
          return request.promise;
        },
      },
      scope,
    ),
  )!;
  try {
    scope.value = { ...first };
    await nextTick();
    assert.equal(calls.length, 1, 'a clock update cannot refetch');
    scope.value = second;
    await nextTick();
    assert.deepEqual(
      calls.map(({ args }) => args),
      [
        ['1', 'a'],
        ['1', 'b'],
      ],
    );
    const current = result(second, 'second');
    calls[1]!.resolve(current);
    calls[0]!.resolve(result(first, 'old'));
    await nextTick();
    assert.equal(model.view.value, current);
    const firstDuplicate = model.target(current, 0)!;
    const secondDuplicate = model.target(current, 1)!;
    assert.notEqual(firstDuplicate.index, secondDuplicate.index);
    assert.equal(model.ownsTarget(secondDuplicate), true);
    assert.equal(model.target(current, 2), undefined);
    assert.equal(model.target(current, 0.5), undefined);
    scope.value = undefined;
    scope.value = second;
    assert.equal(model.ownsTarget(secondDuplicate), false);
    await nextTick();
    const noReceipt = result(second, null);
    calls[2]!.resolve(noReceipt);
    await nextTick();
    assert.equal(model.view.value, noReceipt);
    assert.equal(model.target(noReceipt, 1), undefined);
  } finally {
    lifetime.stop();
  }
});
