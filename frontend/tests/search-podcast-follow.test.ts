import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, nextTick, ref } from 'vue';
import { useSearchPodcastFollow } from '../src/composables/useSearchPodcastFollow.ts';
import { usePodcastFollow } from '../src/composables/usePodcastFollow.ts';
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

test('vinyl collection binds the write to the played episode and hides old completion notices', async (t) => {
  const active = ref(true);
  const target = ref({ playId: 'a', radioId: '301' });
  const writes: PodcastCommand[] = [];
  const write = deferred<AccountData<PodcastMutation>>();
  let reads = 0;
  const scope = effectScope();
  t.after(() => scope.stop());
  const model = scope.run(() =>
    usePodcastFollow(
      {
        async podcastSearchMembership(epoch) {
          reads++;
          return { epoch, value: { subscribedIds: [], pendingIds: [] } };
        },
        podcastCommand(_epoch, command) {
          writes.push(command);
          return write.promise;
        },
      },
      () => account('2'),
      () => active.value,
      () => {},
      {
        scope: () => target.value.playId,
        action(owner, id, current) {
          const captured = target.value;
          if (captured.radioId !== id || current.kind !== 'toggle') return;
          return () => {
            assert.equal(owner, '2');
            writes.push({
              kind: 'change_vinyl_subscription',
              play_id: captured.playId,
              expected: current.subscribed,
            });
            return write.promise;
          };
        },
      },
    ),
  )!;
  await flush();
  const shown = model.control('301')!;
  assert.deepEqual(shown, { kind: 'toggle', subscribed: false, busy: false });
  const change = model.activate('301', shown);
  assert.deepEqual(writes, [{ kind: 'change_vinyl_subscription', play_id: 'a', expected: false }]);
  target.value = { playId: 'b', radioId: '302' };
  await flush();
  assert.equal(reads, 1, 'a new episode uses the same account membership');
  write.resolve({
    epoch: '2',
    value: {
      action: 'subscribe',
      id: '301',
      desired: true,
      observed: true,
      phase: 'confirmed',
      acknowledged: true,
    },
  });
  await change;
  assert.equal(model.status.value, '');
  assert.deepEqual(model.control('301'), { kind: 'toggle', subscribed: true, busy: false });
  await model.activate('301', model.control('301')!);
  assert.equal(writes.length, 1, 'an old radio cannot create a new playback command');
  active.value = false;
  await model.retry();
  assert.equal(reads, 2, 'inactive details cannot reread membership');
});

test('a completion from a replaced native detail receipt cannot overlay fresh membership', async (t) => {
  const receipt = ref('first');
  const membership = deferred<AccountData<PodcastSearchMembership>>();
  let reads = 0;
  const scope = effectScope();
  t.after(() => scope.stop());
  const model = scope.run(() =>
    usePodcastFollow(
      {
        async podcastSearchMembership(epoch) {
          if (++reads === 2) return membership.promise;
          return { epoch, value: { subscribedIds: [], pendingIds: [] } };
        },
        async podcastCommand() {
          throw new Error('unexpected legacy action');
        },
      },
      () => account('2'),
      () => true,
      () => {},
      {
        scope: () => receipt.value,
        receipt: () => receipt.value,
        action: () => async () => ({
          epoch: '2',
          value: {
            action: 'subscribe',
            id: '301',
            desired: true,
            observed: true,
            phase: 'confirmed',
            acknowledged: true,
          },
        }),
      },
    ),
  )!;
  await flush();
  const writing = model.activate('301', model.control('301')!);
  await flush();
  assert.equal(reads, 2);
  assert.equal(model.status.value, '已收藏播客');
  receipt.value = 'second';
  membership.resolve({ epoch: '2', value: { subscribedIds: [], pendingIds: ['301'] } });
  await writing;
  assert.equal(model.status.value, '');
  assert.deepEqual(model.control('301'), { kind: 'reconcile', busy: false });
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
