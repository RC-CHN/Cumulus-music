import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import { useAccountAvatar } from '../src/composables/useAccountAvatar.ts';
import {
  avatarCanAbandon,
  avatarContinue,
  type AccountAvatarClient,
  type AvatarTask,
} from '../src/services/accountAvatar.ts';

const crop = { x: 40, y: 60, size: 240 };
const draft: AvatarTask = {
  id: 'first-task',
  phase: 'draft',
  crop: null,
  width: 640,
  height: 360,
  bytes: 0,
  total: 0,
  transfer: null,
  error: null,
  mayLeaveObject: false,
  cleanupPending: false,
  metadataPending: false,
  observedRevision: null,
  observedAvatarUrl: null,
};
const preview = { width: 640, height: 360, crop, preview: 'data:image/jpeg;base64,AA==' };
function fixture(handlers: Partial<AccountAvatarClient>) {
  const scope = effectScope();
  const owner = ref<string | null>('avatar-account');
  const completed: AvatarTask[] = [];
  const model = scope.run(() =>
    useAccountAvatar(
      {
        accountAvatarTasks: async (epoch) => ({ epoch, value: [] }),
        accountAvatarImage: async (epoch) => ({ epoch, value: preview }),
        pickAccountAvatar: async (epoch) => ({ epoch, value: null }),
        accountAvatarCommand: async () => {
          throw Error('unexpected command');
        },
        ...handlers,
      },
      () => owner.value,
      (task) => completed.push(task),
      () => {},
    ),
  )!;
  return { scope, owner, model, completed };
}

test('a cancelled replacement keeps the existing image and does not abandon it', async () => {
  let picks = 0;
  const f = fixture({
    accountAvatarTasks: async (epoch) => ({ epoch, value: [draft] }),
    pickAccountAvatar: async (epoch) => {
      picks++;
      return { epoch, value: null };
    },
  });
  await f.model.start();
  assert.equal(picks, 0);
  assert.equal(f.model.open.value, true);
  const previous = f.model.image.value;
  await f.model.pick();
  assert.equal(picks, 1);
  assert.equal(f.model.task.value?.id, draft.id);
  assert.equal(f.model.image.value, previous);
  f.scope.stop();
});

test('invalid replacement does not block the existing crop or require recovery', async () => {
  const f = fixture({
    accountAvatarTasks: async (epoch) => ({ epoch, value: [draft] }),
    pickAccountAvatar: async () => {
      throw { code: 'cover_dimensions', message: 'synthetic invalid image' };
    },
  });
  await f.model.start();
  await f.model.pick();
  assert.equal(f.model.readFailed.value, false);
  assert.equal(f.model.task.value?.id, draft.id);
  assert.equal(f.model.image.value, preview);
  assert.match(f.model.error.value, /320/);
  f.scope.stop();
});

test('a lost save receipt recovers the binding task and prevents a second submission', async () => {
  let current = draft;
  const seen: string[] = [];
  const f = fixture({
    accountAvatarTasks: async (epoch) => ({ epoch, value: [current] }),
    accountAvatarCommand: async (epoch, id, command) => {
      assert.equal(id, draft.id);
      seen.push(command.kind);
      if (command.kind === 'save') {
        assert.deepEqual(command.crop, crop);
        current = { ...draft, phase: 'binding', mayLeaveObject: true };
        throw Error('lost IPC');
      }
      if (command.kind === 'reconcile') {
        current = {
          ...current,
          observedRevision: 'observed-synthetic',
          observedAvatarUrl: 'https://p5.music.126.net/synthetic.jpg',
        };
      } else if (command.kind === 'accept_current') {
        assert.equal(command.expectedRevision, current.observedRevision);
        current = { ...current, phase: 'accepted_current_state' };
      }
      return { epoch, value: current };
    },
  });
  await f.model.start();
  await f.model.command({ kind: 'save', crop });
  assert.equal(f.model.readFailed.value, true);
  await f.model.command({ kind: 'save', crop });
  await f.model.reload();
  assert.equal(f.model.task.value?.phase, 'binding');
  await f.model.command({ kind: 'save', crop });
  await f.model.command({ kind: 'abandon' });
  await f.model.command({ kind: 'accept_current', expectedRevision: 'guess' });
  await f.model.command({ kind: 'reconcile' });
  await f.model.command({ kind: 'accept_current', expectedRevision: 'observed-synthetic' });
  assert.deepEqual(seen, ['save', 'reconcile', 'accept_current']);
  assert.equal(f.model.open.value, false);
  assert.equal(f.completed[0]?.phase, 'accepted_current_state');
  f.scope.stop();
});

test('a picker response for a departed account cannot populate a new cropper', async () => {
  let picked!: (result: { epoch: string; value: AvatarTask }) => void;
  let picking!: () => void;
  const started = new Promise<void>((resolve) => (picking = resolve));
  const f = fixture({
    pickAccountAvatar: () => {
      picking();
      return new Promise((resolve) => (picked = resolve));
    },
  });
  const opening = f.model.start();
  await started;
  f.owner.value = 'other-account';
  picked({ epoch: 'avatar-account', value: draft });
  await opening;
  assert.equal(f.model.task.value, undefined);
  assert.equal(f.model.image.value, undefined);
  assert.equal(f.model.open.value, false);
  f.scope.stop();
});

test('ambiguous upload and stale avatar baseline cannot send a new upload or bind', () => {
  assert.equal(avatarContinue({ ...draft, phase: 'uploading', transfer: 'ambiguous' }), false);
  assert.equal(avatarContinue({ ...draft, phase: 'uploaded', error: 'avatar_changed' }), false);
  assert.equal(avatarCanAbandon({ ...draft, phase: 'binding' }), false);
  assert.equal(avatarContinue({ ...draft, phase: 'uploading', transfer: null }), true);
});
