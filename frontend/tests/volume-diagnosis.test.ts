import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, ref } from 'vue';
import { useVolumeDiagnosis } from '../src/composables/useVolumeDiagnosis.ts';

test('held old-account diagnosis cannot hide or consume a new receipt', async () => {
  const scope = effectScope();
  const owner = ref('1');
  const decisions: string[][] = [];
  const checked: string[] = [];
  const releases = new Map<string, (value: string) => void>();
  const model = scope.run(() =>
    useVolumeDiagnosis(
      {
        checkVolumeDiagnosis: ({ epoch }) => {
          checked.push(epoch);
          return new Promise((resolve) => releases.set(epoch, resolve));
        },
        dismissVolumeDiagnosis: async (...args) => {
          decisions.push(args);
        },
      },
      () => owner.value,
      () => assert.fail('test closes the current prompt before timeout'),
    ),
  )!;
  const old = model.check({ epoch: '1', before: 60, after: 80 });
  await model.check({ epoch: '1', before: 80, after: 90 });
  owner.value = '2';
  const current = model.check({ epoch: '2', before: 50, after: 90 });
  releases.get('2')!('new');
  await current;
  releases.get('1')!('old');
  await old;
  assert.deepEqual(checked, ['1', '2']);
  assert.deepEqual(model.prompt.value, { epoch: '2', receipt: 'new' });
  assert.equal(model.close('jump_settings'), '2');
  assert.equal(model.close('manual_close'), undefined);
  assert.deepEqual(decisions, [['2', 'new', 'jump_settings']]);
  scope.stop();
});
