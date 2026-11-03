import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useQuickLoginEligibility } from '../src/composables/useQuickLoginEligibility.ts';
import type { QuickLoginEligibilityView } from '../src/services/quickLoginEligibility.ts';

const eligibility: QuickLoginEligibilityView = {
  accounts: [
    { id: 'saved-current', current: true, eligible: true },
    { id: 'saved-older', current: false, eligible: false },
  ],
  checkedCount: 2,
};

test('the picker does not contact eligibility until explicitly checked and sends only its epoch', async () => {
  const requests: string[] = [];
  const model = useQuickLoginEligibility(
    {
      quickLoginEligibility: async (epoch) => {
        requests.push(epoch);
        return { epoch, value: eligibility };
      },
    },
    () => 'picker-epoch',
  );
  assert.deepEqual(requests, []);
  assert.equal(model.state.value, 'idle');
  await Promise.all([model.check(), model.check()]);
  assert.deepEqual(requests, ['picker-epoch']);
  assert.equal(model.state.value, 'ready');
  assert.deepEqual(model.view.value, eligibility);
  model.reset();
  assert.deepEqual(requests, ['picker-epoch']);
  assert.equal(model.view.value, undefined);
});

test('a late result is suppressed after list invalidation or an owner change', async () => {
  let owner = 'original-epoch';
  let reply!: (value: { epoch: string; value: QuickLoginEligibilityView }) => void;
  const model = useQuickLoginEligibility(
    { quickLoginEligibility: () => new Promise((resolve) => (reply = resolve)) },
    () => owner,
  );
  const beforeRefresh = model.check();
  model.reset();
  reply({ epoch: owner, value: eligibility });
  await beforeRefresh;
  assert.equal(model.state.value, 'idle');
  assert.equal(model.view.value, undefined);

  const beforeSwitch = model.check();
  owner = 'new-epoch';
  reply({ epoch: 'original-epoch', value: eligibility });
  await beforeSwitch;
  assert.equal(model.state.value, 'idle');
  assert.equal(model.view.value, undefined);
});

test('a failed recheck discards the old result and closing blocks further reads', async () => {
  let count = 0;
  const model = useQuickLoginEligibility(
    {
      quickLoginEligibility: async (epoch) => {
        if (++count > 1) throw new Error('synthetic backend error');
        return { epoch, value: eligibility };
      },
    },
    () => 'epoch',
  );
  await model.check();
  assert.equal(model.state.value, 'ready');
  await model.check();
  assert.equal(model.state.value, 'error');
  assert.equal(model.view.value, undefined);
  model.dispose();
  await model.check();
  assert.equal(count, 2);
});
