import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useProfilePodcasts } from '../src/composables/useProfilePodcasts.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type { ProfilePodcastPage, ProfilePodcastQuery } from '../src/services/profilePodcasts.ts';

test('profile podcast replies cannot cross profiles or reappear after a guest denial', async () => {
  const calls: {
    epoch: string;
    uid: string;
    query: ProfilePodcastQuery;
    settle(value: AccountData<ProfilePodcastPage | null>): void;
  }[] = [];
  const model = useProfilePodcasts(
    {
      profilePodcasts: (epoch, uid, query) =>
        new Promise((settle) => calls.push({ epoch, uid, query, settle })),
      openProfilePodcastManagement: async () => assert.fail('unexpected management operation'),
    },
    () => assert.fail('unexpected authentication refresh'),
  );
  const received = {
    rows: [],
    total: 0,
    offset: 0,
    nextOffset: null,
    truncated: false,
  };
  model.connect('10', '73');
  model.ensure();
  model.connect('11', '74');
  model.ensure();
  assert.deepEqual(
    calls.map(({ epoch, uid, query }) => [epoch, uid, query]),
    [
      ['10', '73', { kind: 'collected' }],
      ['10', '73', { kind: 'created', offset: 0 }],
      ['11', '74', { kind: 'collected' }],
      ['11', '74', { kind: 'created', offset: 0 }],
    ],
  );
  calls[0].settle({ epoch: '10', value: received });
  calls[1].settle({ epoch: '10', value: null });
  await Promise.resolve();
  assert.equal(model.groups.collected.page, null);
  assert.equal(model.unavailable.value, false);

  calls[3].settle({ epoch: '11', value: null });
  await Promise.resolve();
  assert.equal(model.unavailable.value, true);
  calls[2].settle({ epoch: '11', value: received });
  await Promise.resolve();
  assert.equal(model.groups.collected.page, null);
  assert.equal(model.groups.created.pending, false);
  model.ensure();
  assert.equal(calls.length, 4);
});
