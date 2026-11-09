import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useLocalDirectories } from '../src/composables/useLocalDirectories.ts';
import type { LocalDirectoryClient, LocalDirectorySelection } from '../src/services/local.ts';

function state(revision: string, entries: [string, boolean][]): LocalDirectorySelection {
  return {
    revision,
    entries: entries.map(([id, checked]) => ({
      id,
      path: '/fixture/' + id,
      checked,
      paused: false,
    })),
  };
}

test('native add stages an unchecked directory without committing existing checkbox edits', async () => {
  const submitted: [string, { id: string; checked: boolean }[]][] = [];
  const core: LocalDirectoryClient = {
    localDirectorySelection: async () => state('initial', [['a', true]]),
    stageLocalDirectory: async (revision) => {
      assert.equal(revision, 'initial');
      return state('staged', [
        ['a', true],
        ['b', false],
      ]);
    },
    applyLocalDirectories: async (revision, choices) => {
      submitted.push([revision, choices]);
      return state('applied', [
        ['a', false],
        ['b', true],
      ]);
    },
  };
  const model = useLocalDirectories(core);
  await model.read();
  model.change('a', false);
  await model.add();
  assert.deepEqual(
    [...model.draft.value],
    [
      ['a', false],
      ['b', false],
    ],
  );
  assert.deepEqual(submitted, []);
  model.change('b', true);
  assert.deepEqual(await model.confirm(), { changed: true });
  assert.deepEqual(submitted, [
    [
      'staged',
      [
        { id: 'a', checked: false },
        { id: 'b', checked: true },
      ],
    ],
  ]);
});

test('stale confirmation requires reread and never reapplies the old draft', async () => {
  let revision = 'before';
  let submits = 0;
  const model = useLocalDirectories({
    localDirectorySelection: async () => state(revision, [['a', revision === 'before']]),
    stageLocalDirectory: async () => null,
    applyLocalDirectories: async () => {
      submits++;
      throw { code: 'snapshot_changed', message: '已变化' };
    },
  });
  await model.read();
  model.change('a', false);
  assert.equal(await model.confirm(), undefined);
  assert.equal(model.stale.value, true);
  assert.equal(model.usable.value, false);
  await model.confirm();
  assert.equal(submits, 1);
  revision = 'after';
  await model.read();
  assert.equal(model.usable.value, true);
  assert.deepEqual([...model.draft.value], [['a', false]]);
});

test('a folder picker that settles after leaving never changes the retired window draft', async () => {
  const late = Promise.withResolvers<LocalDirectorySelection>();
  const model = useLocalDirectories({
    localDirectorySelection: async () => state('before', [['a', true]]),
    stageLocalDirectory: async () => late.promise,
    applyLocalDirectories: async () => assert.fail('no confirmation'),
  });
  await model.read();
  const picking = model.add();
  model.dispose();
  late.resolve(
    state('late', [
      ['a', true],
      ['b', false],
    ]),
  );
  await picking;
  assert.deepEqual([...model.draft.value], [['a', true]]);
});
