import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useProfileFollow } from '../src/composables/useProfileFollow.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type { ProfileFollowView } from '../src/services/profileFollow.ts';

const initial: ProfileFollowView = {
  relation: { followed: false, mutual: false, followersCount: 10 },
  pending: false,
  status: 'ready',
};
const confirmed: ProfileFollowView = {
  relation: { followed: true, mutual: true, followersCount: 12 },
  pending: false,
  status: 'followed',
};

test('a hidden profile can settle its authorized receipt but cannot submit and never crosses accounts', async () => {
  const pending: ((reply: AccountData<ProfileFollowView>) => void)[] = [];
  const applied: string[] = [];
  const model = useProfileFollow(
    { profileFollow: () => new Promise((resolve) => pending.push(resolve)) },
    () => initial,
    () => assert.fail('unexpected account refresh'),
    (target, result) => applied.push(`${target}:${result.status}`),
  );
  const command = { kind: 'change', expected: false } as const;
  model.connect('old', '73');
  const first = model.act(command);
  model.connect('old', null);
  await model.act(command);
  assert.equal(pending.length, 1);
  pending[0]({ epoch: 'old', value: confirmed });
  await first;
  model.connect('old', '73');
  assert.equal(model.active.value?.relation?.mutual, true);

  model.connect('old', '74');
  const delayed = model.act(command);
  model.connect('new', '74');
  pending[1]({ epoch: 'old', value: confirmed });
  await delayed;
  assert.equal(model.active.value?.relation?.followed, false);
  assert.deepEqual(applied, ['73:followed']);
});
