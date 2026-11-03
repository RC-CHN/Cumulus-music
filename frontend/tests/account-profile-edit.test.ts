import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useAccountProfileEdit } from '../src/composables/useAccountProfileEdit.ts';
import {
  localProfileBirthday,
  profileDraft,
  profileDraftChange,
  type AccountProfileEditClient,
  type ProfileEditOperation,
  type ProfileEditPage,
  type ProfileEditSnapshot,
} from '../src/services/accountProfileEdit.ts';

const initial: ProfileEditSnapshot = {
  revision: 'baseline',
  fetchedAt: 0,
  profile: { uid: '100', nickname: 'Existing', signature: 'Old introduction', gender: 0 },
};
const page = (snapshot = initial, operations: ProfileEditOperation[] = []): ProfileEditPage => ({
  snapshot,
  operations,
});
function fixture(epoch: string, client: AccountProfileEditClient) {
  const scope = effectScope();
  const owner = ref<string | null>(epoch);
  const saved: [ProfileEditSnapshot, boolean][] = [];
  const model = scope.run(() =>
    useAccountProfileEdit(
      client,
      () => owner.value,
      (...result) => saved.push(result),
      () => {},
    ),
  )!;
  return { scope, owner, saved, model };
}

test('profile patches preserve omitted service fields and distinguish clearing an introduction', () => {
  const base = profileDraft(initial.profile);
  assert.deepEqual(profileDraftChange(base, { ...base, signature: '' }).patch, { signature: '' });
  assert.deepEqual(profileDraftChange(base, { ...base, nickname: '  Existing ' }).patch, {});
  assert.equal(profileDraftChange(base, { ...base, signature: '汉'.repeat(151) }).valid, false);
  assert.equal(profileDraftChange(base, { ...base, province: 330000 }).valid, false);
  assert.equal(localProfileBirthday('2001-02-29'), undefined);
  assert.deepEqual(localProfileBirthday('2000-02-29'), {
    year: 2000,
    month: 2,
    day: 29,
    utcOffsetMinutes: -new Date(2000, 1, 29).getTimezoneOffset() || 0,
  });
});

test('a conflicting revision keeps changed fields for review without restoring stale untouched fields', async () => {
  let snapshot = initial;
  const f = fixture('rebase', {
    accountProfileEditPage: async (epoch) => ({ epoch, value: page(snapshot) }),
    accountProfileEditCommand: async () => {
      throw { code: 'account_profile_stale', message: 'synthetic conflict' };
    },
  });
  await f.model.read();
  f.model.draft.value!.signature = 'My intended edit';
  await f.model.save();
  assert.equal(f.model.canSave.value, false);
  assert.equal(f.model.refreshRequired.value, true);
  snapshot = {
    ...initial,
    revision: 'newer',
    profile: { ...initial.profile, nickname: 'NewName' },
  };
  await f.model.read();
  assert.equal(f.model.draft.value!.nickname, 'NewName');
  assert.deepEqual(f.model.changes.value!.patch, { signature: 'My intended edit' });
  assert.equal(f.model.canSave.value, true);
  assert.deepEqual(f.saved, []);
  f.scope.stop();
});

test('missing receipt stays blocked across editor reopen and a later journal result is reconciled once', async () => {
  let operation: ProfileEditOperation | undefined;
  let journalVisible = false;
  const written = {
    ...initial,
    revision: 'after',
    profile: { ...initial.profile, signature: 'Updated' },
  };
  const commands: string[] = [];
  const client: AccountProfileEditClient = {
    accountProfileEditPage: async (epoch) => ({
      epoch,
      value: page(initial, journalVisible && operation ? [operation] : []),
    }),
    accountProfileEditCommand: async (epoch, command) => {
      commands.push(command.kind);
      if (command.kind === 'change') {
        operation = {
          ...command,
          submitted: command.patch,
          phase: 'uncertain',
          createdAt: 1,
          observed: null,
        };
        throw Error('transport reply lost');
      }
      assert.equal(command.id, operation!.id);
      return {
        epoch,
        value: page(written, [{ ...operation!, phase: 'confirmed', observed: written }]),
      };
    },
  };
  const first = fixture('unknown', client);
  await first.model.read();
  first.model.draft.value!.signature = 'Updated';
  await first.model.save();
  assert.equal(first.model.canSave.value, false);
  first.scope.stop();

  const reopened = fixture('unknown', client);
  await reopened.model.read();
  assert.equal(reopened.model.unseen.value, operation!.id);
  await reopened.model.save();
  assert.deepEqual(commands, ['change']);

  journalVisible = true;
  await reopened.model.reconcile();
  assert.equal(reopened.model.blocked.value?.id, operation!.id);
  await reopened.model.reconcile();
  assert.deepEqual(commands, ['change', 'reconcile']);
  assert.deepEqual(reopened.saved, [[written, true]]);
  reopened.scope.stop();
});

test('native preflight rejection keeps the draft editable and a stale reply cannot announce success', async () => {
  let resolveLate!: (value: { epoch: string; value: ProfileEditPage }) => void;
  const f = fixture('prior-owner', {
    accountProfileEditPage: async (epoch) => ({ epoch, value: page() }),
    accountProfileEditCommand: async (_epoch, command) => {
      if (command.kind !== 'change') throw new Error('unexpected command');
      if (command.patch.nickname) {
        throw { code: 'profile_nickname_taken', message: 'synthetic taken' };
      }
      return new Promise((resolve) => (resolveLate = resolve));
    },
  });
  await f.model.read();
  f.model.draft.value!.nickname = 'UnavailableName';
  await f.model.save();
  assert.equal(f.model.nicknameTaken.value, true);
  assert.equal(f.model.unseen.value, undefined);
  assert.equal(f.model.draft.value!.nickname, 'UnavailableName');

  f.model.draft.value!.nickname = initial.profile.nickname!;
  f.model.draft.value!.signature = 'Edited';
  assert.equal(f.model.canSave.value, true);
  const saving = f.model.save();
  f.owner.value = 'next-owner';
  resolveLate({ epoch: 'prior-owner', value: page() });
  await saving;
  assert.deepEqual(f.saved, []);
  f.scope.stop();
});

test('an existing native journal blocks a new save without creating an unseen intent', async () => {
  const previous: ProfileEditOperation = {
    id: 'prior-operation',
    expectedRevision: initial.revision,
    patch: { signature: 'Other edit' },
    submitted: { signature: 'Other edit' },
    phase: 'uncertain',
    createdAt: 0,
    observed: null,
  };
  let journal: ProfileEditOperation[] = [];
  const f = fixture('existing-write', {
    accountProfileEditPage: async (epoch) => ({ epoch, value: page(initial, journal) }),
    accountProfileEditCommand: async () => {
      journal = [previous];
      throw { code: 'write_uncertain', message: 'prior write unresolved' };
    },
  });
  await f.model.read();
  f.model.draft.value!.signature = 'Keep this draft';
  await f.model.save();
  assert.equal(f.model.unseen.value, undefined);
  assert.equal(f.model.blocked.value?.id, previous.id);
  assert.equal(f.model.draft.value!.signature, 'Keep this draft');
  assert.equal(f.model.canSave.value, false);
  f.scope.stop();
});
