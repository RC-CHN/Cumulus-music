import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { effectScope, ref, shallowRef } from 'vue';
import { useCloudDelete } from '../src/composables/useCloudDelete.ts';
import { cloudDeleteTarget, cloudDeleteWarning } from '../src/services/cloudDelete.ts';
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
test('confirmation and the actual command keep the same captured rows even if the source selection or target is mutated', async () => {
  const rows: any[] = [{ track: { id: '7', name: 'Original' }, deletionKind: 'private' }];
  const input = cloudDeleteTarget('1', 'r', rows);
  rows[0].track.id = '99';
  rows[0].track.name = 'Changed';
  rows[0].deletionKind = 'ordinary';
  rows.push({ track: { id: '100', name: 'New' } });
  let sent: any,
    closes = 0;
  const model: any = {
    accountEpoch: ref('1'),
    view: shallowRef({ revision: 'r', deletions: [] }),
    busy: ref(false),
    command: async (value: any) => {
      sent = value;
      return {
        deletion: { id: 'op', ids: ['7'], phase: 'rejected', total: 1, removed: 0, remaining: 1 },
      };
    },
  };
  const scope = effectScope(),
    state = scope.run(() => useCloudDelete(model, input, () => closes++))!;
  input.ids[0] = '999';
  input.titles[0] = 'Mutated dialog input';
  input.kinds![0] = 'ordinary';
  input.revision = 'changed';
  assert.deepEqual(state.target.ids, ['7']);
  assert.deepEqual(state.target.titles, ['Original']);
  assert.equal(
    cloudDeleteWarning(state.target),
    oracle.groups.find((v: any) => v.name === 'private_priority').confirmation.message,
  );
  await state.send();
  assert.deepEqual(sent, { kind: 'delete', ids: ['7'], revision: 'r' });
  assert.equal(closes, 0);
  assert.equal(state.canSend.value, false);
  scope.stop();
});
test('an empty, oversized, duplicated or incomplete confirmation cannot dispatch a deletion', async () => {
  for (const ids of [
    [],
    ['1', '1'],
    ['1', '2'],
    Array.from({ length: 5001 }, (_, i) => String(i + 1)),
  ]) {
    let writes = 0;
    const model: any = {
      accountEpoch: ref('1'),
      view: shallowRef({ revision: 'r', deletions: [] }),
      busy: ref(false),
      command: async () => writes++,
    };
    const scope = effectScope(),
      state = scope.run(() =>
        useCloudDelete(
          model,
          { epoch: '1', revision: 'r', ids, titles: ids.length === 2 ? ['One'] : ids.map(String) },
          () => {},
        ),
      )!;
    await state.send();
    assert.equal(writes, 0);
    assert.equal(state.canSend.value, false);
    scope.stop();
  }
});
