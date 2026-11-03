import { test } from 'node:test';
import assert from 'node:assert/strict';
import { shallowRef } from 'vue';
import { useEqualizerDraft } from '../src/composables/useEqualizerDraft.ts';
import type { EqualizerSettings, EqualizerView, PlayerAction } from '../src/services/player.ts';

type Write = Extract<PlayerAction, { kind: 'equalizer' }>;
const curve = (gain: number): EqualizerSettings => ({
  enabled: true,
  gains: [gain, ...Array<number>(9).fill(0)],
});
const snapshot = (epoch: string, revision: string, settings = curve(0)) => ({
  epoch,
  equalizer: { revision, settings, damaged: false, status: 'configured' } satisfies EqualizerView,
});
const tick = () => new Promise<void>((resolve) => setTimeout(resolve, 5));
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

test('audition sends before release, coalesces a burst, and persists the final position', async () => {
  const current = shallowRef(snapshot('A', '10'));
  const first = deferred<typeof current.value>();
  const writes: Write[] = [];
  const draft = useEqualizerDraft(
    () => current.value,
    async (command) => {
      writes.push(command);
      if (writes.length === 1) return (current.value = await first.promise);
      return (current.value = snapshot('A', '12', command.settings));
    },
    async () => current.value,
    1,
  );
  draft.stage(curve(1));
  draft.stage(curve(2));
  await tick();
  assert.deepEqual(
    writes.map((item) => [item.expected_revision, item.settings.gains[0]]),
    [['10', 2]],
  );
  draft.stage(curve(3));
  draft.stage(curve(4));
  await tick();
  assert.equal(writes.length, 1);
  assert.equal(draft.settings.value?.gains[0], 4);
  first.resolve(snapshot('A', '11', curve(2)));
  await tick();
  assert.deepEqual(
    writes.map((item) => [item.expected_revision, item.settings.gains[0]]),
    [
      ['10', 2],
      ['11', 4],
    ],
  );
  assert.equal(draft.draft.value, undefined);
  assert.equal(draft.settings.value?.gains[0], 4);
});

test('conflict retains the latest draft and only explicit fresh read can rebase it', async () => {
  const current = shallowRef(snapshot('A', '10'));
  const writes: string[] = [];
  let reads = 0;
  const draft = useEqualizerDraft(
    () => current.value,
    async (command) => {
      writes.push(command.expected_revision);
      if (writes.length === 1)
        throw { code: 'equalizer_changed', message: '版本冲突', retryable: true };
      return (current.value = snapshot('A', '21', command.settings));
    },
    async () => {
      reads++;
      return (current.value = snapshot('A', '20', curve(7)));
    },
    1,
  );
  draft.stage(curve(2));
  await tick();
  draft.stage(curve(4));
  await tick();
  assert.deepEqual(writes, ['10']);
  assert.equal(reads, 0);
  assert.equal(draft.blocked.value, true);
  assert.equal(draft.settings.value?.gains[0], 4);
  assert.match(draft.error.value, /版本冲突/);
  await draft.applyDraft();
  assert.equal(reads, 1);
  assert.deepEqual(writes, ['10', '20']);
  assert.equal(draft.blocked.value, false);
  assert.equal(current.value.equalizer.settings.gains[0], 4);
});

test('lost acknowledgment is settled by readback without writing again', async () => {
  const current = shallowRef(snapshot('A', '10'));
  let writes = 0;
  const draft = useEqualizerDraft(
    () => current.value,
    async () => {
      writes++;
      throw new Error('IPC reply lost');
    },
    async () => (current.value = snapshot('A', '11', curve(Math.fround(1.79)))),
    1,
  );
  draft.stage(curve(1.79));
  await tick();
  assert.equal(draft.blocked.value, true);
  await draft.applyDraft();
  assert.equal(writes, 1);
  assert.equal(draft.blocked.value, false);
  assert.equal(draft.draft.value, undefined);
});

test('epoch change drops queued curve and ignores an older in-flight rejection', async () => {
  const current = shallowRef(snapshot('A', '10'));
  const held = deferred<typeof current.value>();
  const writes: [string, number][] = [];
  const draft = useEqualizerDraft(
    () => current.value,
    async (command) => {
      writes.push([command.expected_revision, command.settings.gains[0]!]);
      if (writes.length === 1) return held.promise;
      return (current.value = snapshot('B', '21', command.settings));
    },
    async () => current.value,
    1,
  );
  draft.stage(curve(1));
  await tick();
  draft.stage(curve(2));
  current.value = snapshot('B', '20');
  assert.equal(draft.draft.value, undefined);
  draft.stage(curve(8));
  held.reject(new Error('old failure'));
  await tick();
  assert.deepEqual(writes, [
    ['10', 1],
    ['20', 8],
  ]);
  assert.equal(draft.error.value, '');
  assert.equal(draft.settings.value?.gains[0], 8);
});

test('a mismatched write acknowledgment does not cause an automatic second write', async () => {
  const current = shallowRef(snapshot('A', '10'));
  let writes = 0;
  const draft = useEqualizerDraft(
    () => current.value,
    async (command) => {
      writes++;
      return snapshot('B', '11', command.settings);
    },
    async () => current.value,
    1,
  );
  draft.stage(curve(4));
  await tick();
  await tick();
  assert.equal(writes, 1);
  assert.equal(draft.blocked.value, true);
  assert.equal(draft.settings.value?.gains[0], 4);
  draft.discard();
  assert.equal(draft.settings.value?.gains[0], 0);
});

test('tone edits coalesce into the same acknowledged curve and reset independently', async () => {
  const current = shallowRef(snapshot('A', '10', curve(3)));
  const writes: Write[] = [];
  const draft = useEqualizerDraft(
    () => current.value,
    async (command) => {
      writes.push(command);
      return (current.value = snapshot('A', String(10 + writes.length), command.settings));
    },
    async () => current.value,
    1,
  );
  draft.stage({ ...curve(3), bass_db: 6 });
  draft.stage({ ...curve(3), bass_db: 6, treble_db: -3 });
  await tick();
  assert.deepEqual(
    writes.map((c) => c.settings),
    [{ ...curve(3), bass_db: 6, treble_db: -3 }],
  );
  assert.equal(draft.draft.value, undefined);
  draft.stage({ ...curve(3), bass_db: 0, treble_db: 0 });
  await tick();
  assert.deepEqual(writes[1]?.settings, curve(3));
  assert.equal(draft.draft.value, undefined);
});

test('matching gains alone cannot acknowledge a manual preset selection', async () => {
  const current = snapshot('A', '10');
  const writes: Write[] = [];
  const draft = useEqualizerDraft(
    () => current,
    async (command) => {
      writes.push(command);
      return snapshot('A', '11', command.settings);
    },
    async () => current,
    1,
  );
  draft.stage(curve(0), { kind: 'manual' });
  await tick();
  assert.equal(writes.length, 1);
  assert.deepEqual(writes[0]?.preset, { kind: 'manual' });
  assert.equal(draft.blocked.value, true);
  assert.deepEqual(draft.preset.value, { kind: 'manual' });
});
