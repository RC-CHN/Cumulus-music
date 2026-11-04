import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref } from 'vue';
import { useSearchPodcastFollow } from '../src/composables/useSearchPodcastFollow.ts';
import type { AccountData, AccountView } from '../src/services/contracts.ts';
import type {
  PodcastCommand,
  PodcastMutation,
  PodcastSearchMembership,
} from '../src/services/podcast.ts';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => (resolve = done));
  return { promise, resolve };
}
const flush = async () => {
  await nextTick();
  await nextTick();
};
const account = (epoch: string): AccountView => ({
  epoch,
  status: 'authenticated',
  nickname: null,
  avatarUrl: null,
});

test('search podcast membership discards a different account and refreshes only after explicit intent', async (t) => {
  const active = ref(false);
  const owner = ref(account('1'));
  const reads: {
    epoch: string;
    deferred: ReturnType<typeof deferred<AccountData<PodcastSearchMembership>>>;
  }[] = [];
  const writes: [string, PodcastCommand][] = [];
  const write = deferred<AccountData<PodcastMutation>>();
  const scope = effectScope();
  t.after(() => scope.stop());
  const model = scope.run(() =>
    useSearchPodcastFollow(
      {
        podcastSearchMembership(epoch) {
          const reply = deferred<AccountData<PodcastSearchMembership>>();
          reads.push({ epoch, deferred: reply });
          return reply.promise;
        },
        podcastCommand(epoch, command) {
          writes.push([epoch, command]);
          return write.promise;
        },
      },
      () => owner.value,
      () => active.value,
      () => {},
    ),
  )!;

  assert.deepEqual(reads, []);
  active.value = true;
  await flush();
  assert.equal(model.control('301'), undefined);
  owner.value = account('2');
  await flush();
  reads[1]!.deferred.resolve({ epoch: '2', value: { subscribedIds: ['301'], pendingIds: [] } });
  await flush();
  reads[0]!.deferred.resolve({ epoch: '1', value: { subscribedIds: [], pendingIds: [] } });
  await flush();
  assert.deepEqual(model.control('301'), { kind: 'toggle', subscribed: true, busy: false });
  assert.deepEqual(model.control('302'), { kind: 'toggle', subscribed: false, busy: false });
  assert.deepEqual(
    reads.map((read) => read.epoch),
    ['1', '2'],
  );
  await model.activate('301', { kind: 'toggle', subscribed: false, busy: false });
  assert.deepEqual(writes, []);

  const change = model.activate('301', { kind: 'toggle', subscribed: true, busy: false });
  assert.deepEqual(writes, [
    ['2', { kind: 'change_search_subscription', id: '301', expected: true }],
  ]);
  assert.equal(model.control('301')?.busy, true);
  owner.value = account('3');
  await flush();
  reads[2]!.deferred.resolve({ epoch: '3', value: { subscribedIds: null, pendingIds: [] } });
  write.resolve({
    epoch: '2',
    value: {
      action: 'subscribe',
      id: '301',
      desired: false,
      observed: false,
      phase: 'confirmed',
      acknowledged: true,
    },
  });
  await change;
  assert.equal(model.control('301'), undefined);
  assert.equal(model.status.value, '');
  assert.deepEqual(
    reads.map((read) => read.epoch),
    ['1', '2', '3'],
  );
});
