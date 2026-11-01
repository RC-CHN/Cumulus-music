import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { effectScope, ref, shallowRef } from 'vue';
import { useCloudDelete } from '../src/composables/useCloudDelete.ts';
import { cloudDeleteTarget, cloudDeleteWarning } from '../src/services/cloudDelete.ts';
import { fixture } from './cloud-delete-fixture.ts';
const oracle = JSON.parse(
  readFileSync(new URL('../../core/tests/fixtures/cloud-delete-ui.json', import.meta.url), 'utf8'),
);
test('cloud confirmation uses the executed official private/off-store priority, with an explicit unknown-cache boundary', () => {
  for (const sample of oracle.groups) {
    const target = cloudDeleteTarget(
      '1',
      'r',
      sample.kinds.map((deletionKind: string, index: number) => ({
        track: { id: String(index + 1), name: `Song ${index + 1}` },
        deletionKind,
      })),
    );
    assert.equal(cloudDeleteWarning(target), sample.confirmation.message);
  }
  assert(
    cloudDeleteWarning({ epoch: '1', revision: 'r', ids: ['1'], titles: ['Old cache'] }).includes(
      '类型尚未核实',
    ),
  );
  assert(
    cloudDeleteWarning(
      cloudDeleteTarget('1', 'r', [{ track: { id: '1', name: 'Missing evidence' } } as any]),
    ).includes('类型尚未核实'),
  );
});
test('confirmation, native preview and accepted plan preserve the same captured context', async () => {
  const f = await fixture();
  const original = [...f.state.target.ids];
  f.input.ids[0] = '999';
  f.input.titles[0] = 'Mutated';
  f.input.revision = 'changed';
  await f.state.prepare();
  await f.state.send();
  assert.deepEqual(f.state.target.ids, original);
  assert.deepEqual(f.calls[0], ['prepare', '1', ['7'], 'r1']);
  assert.deepEqual(f.calls[1], ['execute', '1', 'ours', false, false]);
  f.dispose();
});
test('empty, oversized, duplicated or incomplete scopes cannot even create a native preview', async () => {
  for (const ids of [
    [],
    ['1', '1'],
    ['1', '2'],
    Array.from({ length: 5001 }, (_, i) => String(i + 1)),
  ]) {
    let calls = 0;
    const model: any = {
      accountEpoch: ref('1'),
      view: shallowRef({ revision: 'r', deletions: [] }),
      busy: ref(false),
      deletePrepare: async () => calls++,
    };
    const scope = effectScope(),
      state = scope.run(() =>
        useCloudDelete(
          model,
          { epoch: '1', revision: 'r', ids, titles: ids.length === 2 ? ['One'] : ids.map(String) },
          () => {},
        ),
      )!;
    await state.prepare();
    await state.send();
    assert.equal(calls, 0);
    assert(!state.canSend.value);
    scope.stop();
  }
});
