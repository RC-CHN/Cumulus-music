import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePodcastHome } from '../src/composables/usePodcastHome.ts';
import type { CoreClient } from '../src/services/contracts.ts';

const account = {
  epoch: '1',
  status: 'authenticated' as const,
  nickname: '合成账号',
  avatarUrl: null,
};
type Reply = Awaited<ReturnType<CoreClient['podcastHome']>>;
function response(enabled: boolean, cursor: string | null = null): Reply {
  return {
    epoch: '1',
    value: {
      recommendationEnabled: enabled,
      continuation: cursor,
      blocks: enabled ? [{ kind: 'radios', title: '推荐播客', cards: [] }] : [],
    },
  };
}

test('a disabled continuation removes the earlier recommendation blocks', async () => {
  const calls: (string | undefined)[] = [];
  const model = usePodcastHome(
    () => account,
    () => {},
    {
      podcastHome: async (_epoch, cursor) => {
        calls.push(cursor);
        return cursor ? response(false) : response(true, 'first');
      },
    },
  );
  await model.load();
  assert.equal(model.blocks.value.length, 1);
  await model.load();
  assert.equal(model.enabled.value, false);
  assert.deepEqual(model.blocks.value, []);
  assert.equal(model.continuation.value, undefined);
  await model.load();
  assert.deepEqual(calls, [undefined, 'first']);
});

test('a confirmed local opt-out rejects late results and reenable starts without the old cursor', async () => {
  let settle!: (value: Reply) => void;
  const held = new Promise<Reply>((resolve) => (settle = resolve));
  const calls: (string | undefined)[] = [];
  const model = usePodcastHome(
    () => account,
    () => {},
    {
      podcastHome: async (_epoch, cursor) => {
        calls.push(cursor);
        return cursor ? held : response(true, 'first');
      },
    },
  );
  await model.load();
  const next = model.load();
  model.observePersonalService({ epoch: '0', value: 2 });
  assert.equal(model.enabled.value, true);
  model.observePersonalService({ epoch: '1', value: 2 });
  assert.equal(model.enabled.value, false);
  assert.equal(model.pending.value, false);
  settle(response(true, 'late'));
  await next;
  assert.equal(model.enabled.value, false);
  assert.deepEqual(model.blocks.value, []);
  model.observePersonalService({ epoch: '1', value: 0 });
  await model.load();
  assert.deepEqual(calls, [undefined, 'first', undefined]);
  assert.equal(model.enabled.value, true);
});
