import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, shallowRef } from 'vue';
import { usePlaybackChorus } from '../src/composables/usePlaybackChorus.ts';
import type { PlayerView } from '../src/services/player.ts';

test('queue refresh keeps the current chorus point until a fresh reply, never shows an old reply', async () => {
  const timeline = { mode: 'full', start: 0, end: 100, total: 100, length: 100, media_start: 0 };
  const view = shallowRef({
    epoch: '1',
    playId: 'first',
    resourceKind: 'catalog',
    queueRevision: '1',
    queueLen: 2,
    timeline,
  } as PlayerView);
  const scope = effectScope();
  const requests: { epoch: string; playId: string; complete: (seconds: number | null) => void }[] =
    [];
  const marker = scope.run(() =>
    usePlaybackChorus(view, shallowRef(true), {
      playerChorus(epoch, playId) {
        return new Promise((complete) => requests.push({ epoch, playId, complete }));
      },
    }),
  )!;
  try {
    requests[0]!.complete(20);
    await nextTick();
    assert.equal(marker.value, 20);
    view.value = { ...view.value, queueRevision: '2', queueLen: 3 };
    await nextTick();
    assert.equal(marker.value, 20);
    assert.equal(requests.length, 2);

    view.value = { ...view.value, playId: 'second' };
    await nextTick();
    requests[1]!.complete(80);
    await nextTick();
    assert.equal(marker.value, null);
    requests[2]!.complete(50);
    await nextTick();
    assert.equal(marker.value, 50);

    view.value = {
      ...view.value,
      epoch: '2',
      timeline: {
        ...timeline,
        mode: 'trial',
        start: 20,
        end: 40,
        length: 20,
      } as PlayerView['timeline'],
    };
    await nextTick();
    assert.equal(marker.value, null);
    assert.deepEqual(
      requests.map(({ epoch, playId }) => [epoch, playId]),
      [
        ['1', 'first'],
        ['1', 'first'],
        ['1', 'second'],
        ['2', 'second'],
      ],
    );
    requests[3]!.complete(40); // exclusive upper trial marker bound
    await nextTick();
    assert.equal(marker.value, null);
  } finally {
    scope.stop();
  }
});
