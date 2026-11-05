import assert from 'node:assert/strict';
import { test } from 'node:test';
import { effectScope, ref } from 'vue';
import {
  useVinylCommentComposer,
  type VinylComposerTarget,
} from '../src/composables/useVinylCommentComposer.ts';
import type { CommentItem } from '../src/services/comments.ts';
import type { CommentWrite, VinylCommentWriteClient } from '../src/services/commentWrites.ts';

const a: VinylComposerTarget = {
  epoch: '1',
  playId: 'play-a',
  resource: { kind: 'track', id: '61' },
  title: 'Song A',
};
const b: VinylComposerTarget = {
  epoch: '2',
  playId: 'play-b',
  resource: { kind: 'track', id: '62' },
  title: 'Song B',
};
function item(id: string): CommentItem {
  return {
    id,
    content: id,
    time: 0,
    timeText: '',
    liked: false,
    likedCount: 0,
    author: null,
    replies: [],
    owned: false,
  };
}
function operation(
  context: VinylComposerTarget,
  phase: CommentWrite['phase'],
  command: CommentWrite['command'] = { kind: 'publish', content: ' body ', replyTo: null },
): CommentWrite {
  const observed = phase === 'confirmed' ? item('901') : null;
  return {
    id: 'journal-01',
    resource: context.resource,
    command,
    phase,
    createdAt: 0,
    acknowledged: !!observed,
    receiptId: observed?.id ?? null,
    target: null,
    observed,
    observedAbsent: false,
    serviceCode: null,
  };
}
function mock(overrides: Partial<VinylCommentWriteClient>): VinylCommentWriteClient {
  return {
    playerVinylWriteOperations: async (epoch) => ({ epoch, value: [] }),
    playerVinylPrepareWrite: async () => {
      throw Error('unexpected prepare');
    },
    playerVinylWriteCommand: async () => {
      throw Error('unexpected submit');
    },
    ...overrides,
  };
}
async function letReadFinish() {
  await Promise.resolve();
  await Promise.resolve();
}

test('one explicit reply freezes the play, comment identity and untrimmed body', async () => {
  const scope = effectScope();
  try {
    const target = ref<VinylComposerTarget | undefined>(a);
    const committed: string[] = [];
    const calls: unknown[] = [];
    const command = { kind: 'publish', content: ' body ', replyTo: '89' } as const;
    const composer = scope.run(() =>
      useVinylCommentComposer(
        mock({
          playerVinylPrepareWrite: async (epoch, play, input) => {
            calls.push([epoch, play, input]);
            return { epoch, value: operation(a, 'prepared', input) };
          },
          playerVinylWriteCommand: async (epoch, play, id, action) => {
            calls.push([epoch, play, id, action]);
            return { epoch, value: operation(a, 'confirmed', command) };
          },
        }),
        target,
        (scope, row) => committed.push(`${scope.playId}:${row.id}`),
      ),
    )!;
    composer.open(a, item('89'));
    await letReadFinish();
    const writer = composer.current.value!.writer;
    writer.draft.value = command.content;
    await writer.publish();
    assert.deepEqual(calls, [
      ['1', 'play-a', command],
      ['1', 'play-a', 'journal-01', { kind: 'submit' }],
    ]);
    assert.deepEqual(committed, ['play-a:901']);
    assert.equal(composer.current.value, undefined);
  } finally {
    scope.stop();
  }
});

test('hiding and reopening the same play before prepare returns cannot submit the old intent', async () => {
  const scope = effectScope();
  try {
    const target = ref<VinylComposerTarget | undefined>(a);
    let release!: (value: { epoch: string; value: CommentWrite }) => void;
    let submits = 0;
    const composer = scope.run(() =>
      useVinylCommentComposer(
        mock({
          playerVinylPrepareWrite: () =>
            new Promise((resolve) => {
              release = resolve;
            }),
          playerVinylWriteCommand: async (epoch) => {
            submits++;
            return { epoch, value: operation(a, 'confirmed') };
          },
        }),
        target,
        () => assert.fail('stale composition was confirmed'),
      ),
    )!;
    composer.open(a);
    const original = composer.current.value!;
    original.writer.draft.value = ' body ';
    const old = original.writer.publish();
    target.value = undefined;
    target.value = a;
    composer.open(a);
    const replacement = composer.current.value!;
    replacement.writer.draft.value = 'new draft';
    release({ epoch: '1', value: operation(a, 'prepared') });
    await old;
    assert.equal(submits, 0);
    assert.equal(composer.current.value, replacement);
    assert.equal(replacement.writer.draft.value, 'new draft');
  } finally {
    scope.stop();
  }
});

test('an already dispatched write cannot insert its late confirmation into another account', async () => {
  const scope = effectScope();
  try {
    const target = ref<VinylComposerTarget | undefined>(a);
    let finish!: (result: { epoch: string; value: CommentWrite }) => void;
    let started!: () => void;
    const sent = new Promise<void>((resolve) => {
      started = resolve;
    });
    const seen: string[] = [];
    const composer = scope.run(() =>
      useVinylCommentComposer(
        mock({
          playerVinylPrepareWrite: async (epoch) => ({ epoch, value: operation(a, 'prepared') }),
          playerVinylWriteCommand: async () => {
            started();
            return new Promise((resolve) => {
              finish = resolve;
            });
          },
        }),
        target,
        (_, item) => seen.push(item.id),
      ),
    )!;
    composer.open(a);
    const old = composer.current.value!;
    old.writer.draft.value = ' body ';
    const submission = old.writer.publish();
    await sent;
    target.value = b;
    composer.open(b);
    const active = composer.current.value!;
    finish({ epoch: '1', value: operation(a, 'confirmed') });
    await submission;
    assert.deepEqual(seen, []);
    assert.equal(composer.current.value, active);
    assert.equal(active.target.playId, 'play-b');
  } finally {
    scope.stop();
  }
});

test('pending journal rows are read-only here even if an old submit callback is replayed', async () => {
  const scope = effectScope();
  try {
    const target = ref<VinylComposerTarget | undefined>(a);
    const inherited = operation(a, 'prepared');
    let writes = 0;
    const composer = scope.run(() =>
      useVinylCommentComposer(
        mock({
          playerVinylWriteOperations: async (epoch) => ({ epoch, value: [inherited] }),
          playerVinylWriteCommand: async (epoch) => {
            writes++;
            return { epoch, value: operation(a, 'confirmed') };
          },
        }),
        target,
        () => assert.fail('inherited journal was submitted'),
      ),
    )!;
    composer.open(a);
    const writer = composer.current.value!.writer;
    await letReadFinish();
    assert.equal(writer.pending.value[0]?.id, inherited.id);
    await writer.action(inherited, { kind: 'submit' });
    await writer.action(inherited, { kind: 'reconcile' });
    assert.equal(writes, 0);
    assert.equal(writer.pending.value[0]?.phase, 'prepared');
  } finally {
    scope.stop();
  }
});
