import assert from 'node:assert/strict';
import { test } from 'node:test';
import { useProfileRelations } from '../src/composables/useProfileRelations.ts';
import type { AccountData } from '../src/services/contracts.ts';
import type {
  ProfileRelationAccess,
  ProfileRelationCard,
  ProfileRelationKind,
  ProfileRelationPage,
  ProfileRelationTarget,
} from '../src/services/profileRelations.ts';

const target = (owner: string, relationVisit = 1, isSelf = false): ProfileRelationTarget => ({
  id: '81',
  name: '合成用户',
  mode: 'following',
  isSelf,
  owner,
  relationVisit,
});
const card = (id: string): ProfileRelationCard => ({
  id,
  kind: 'user',
  name: id,
  remark: null,
  avatarUrl: null,
  signature: null,
  relationship: null,
});
const response = (
  epoch: string,
  id: string,
  continuation: string | null,
): AccountData<ProfileRelationPage> => ({
  epoch,
  value: { state: 'ready', items: [card(id)], continuation, recordCount: 2 },
});

test('relation continuation retries the same opaque handle and retains the original visit and focus', async () => {
  let fail = true;
  const calls: [ProfileRelationKind, string | null][] = [];
  const model = useProfileRelations(
    {
      profileRelationAccess: async (epoch) => ({
        epoch,
        value: { state: 'allowed', isSelf: false },
      }),
      profileRelationPage: async (epoch, _id, kind, handle) => {
        calls.push([kind, handle]);
        if (!handle) return response(epoch, '1', 'handle');
        if (fail) throw Error('fixture');
        return response(epoch, '2', null);
      },
    },
    () => {},
  );
  await model.enter('a', target('a'));
  await model.load();
  assert.equal(model.list.value?.failed, true);
  assert.deepEqual(
    model.list.value?.items.map((item) => item.id),
    ['1'],
  );
  fail = false;
  await model.load();
  assert.deepEqual(
    model.list.value?.items.map((item) => item.id),
    ['1', '2'],
  );
  model.remember(205, 1);
  model.enter('a', null);
  await model.enter('a', target('a'));
  assert.equal(model.list.value?.top, 205);
  assert.equal(model.list.value?.focus, 1);
  assert.deepEqual(calls, [
    ['following', null],
    ['following', 'handle'],
    ['following', 'handle'],
  ]);
});

test('account changes fence old pages, and own filters keep independent prefixes', async () => {
  let release!: (data: AccountData<ProfileRelationPage>) => void;
  const calls: string[] = [];
  const model = useProfileRelations(
    {
      profileRelationAccess: async (epoch) => ({
        epoch,
        value: { state: 'allowed', isSelf: true },
      }),
      profileRelationPage: (epoch, _id, kind) => {
        calls.push(`${epoch}:${kind}`);
        if (epoch === 'old')
          return new Promise((resolve) => {
            release = resolve;
          });
        return Promise.resolve(response(epoch, kind, null));
      },
    },
    () => {},
  );
  const late = model.enter('old', target('old', 2, true));
  model.enter('new', target('old', 2, true));
  assert.equal(model.visit.value, undefined);
  await model.enter('new', target('new', 3, true));
  model.select('following_users');
  await Promise.resolve();
  assert.equal(model.list.value?.items[0]?.id, 'following_users');
  model.select('following');
  release(response('old', 'private', null));
  await late;
  assert.equal(model.list.value?.items[0]?.id, 'following');
  assert.deepEqual(calls, ['old:following', 'new:following', 'new:following_users']);
});

test('a departed permission check cannot open a list and a current denial remains on the source', async () => {
  let release!: (data: AccountData<ProfileRelationAccess>) => void;
  const model = useProfileRelations(
    {
      profileRelationAccess: (epoch, uid) =>
        uid === 'late'
          ? new Promise((resolve) => {
              release = resolve;
            })
          : Promise.resolve({ epoch, value: { state: 'denied', message: '合成拒绝' } }),
      profileRelationPage: async () => {
        throw Error('must not read the list');
      },
    },
    () => {},
  );
  const late = model.check('a', 'late');
  model.clearGate();
  release({ epoch: 'a', value: { state: 'allowed', isSelf: false } });
  assert.equal(await late, undefined);
  assert.equal(await model.check('a', '81'), undefined);
  assert.equal(model.entryNotice.value, '合成拒绝');
});
