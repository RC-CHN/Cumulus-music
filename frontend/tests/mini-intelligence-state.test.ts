import { test } from 'node:test';
import assert from 'node:assert/strict';
import { effectScope, shallowRef } from 'vue';
import { useMiniModes } from '../src/composables/useMiniModes.ts';
import type {
  MiniClient,
  MiniView,
  MiniPlaybackReceipt,
  MiniIntelligenceCommand,
} from '../src/services/mini.ts';
const turn = () => new Promise((r) => setImmediate(r));
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void;
  const promise = new Promise<T>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const view = shallowRef<MiniView | null>({
    generation: '1',
    ticket: '1',
    preferences: { revision: 'settings', alwaysOnTop: true, damaged: false },
    media: {
      epoch: '1',
      selection: { audioPlayId: 'old', audioEntryId: 'entry', videoPlayId: null },
      item: { id: '7', kind: 'music', title: 'Synthetic', artists: [], album: 'Synthetic' },
      state: 'paused',
      playing: false,
      canPrevious: true,
      canNext: true,
      position: 10,
      duration: 120,
      seekStart: 0,
      seekEnd: 120,
      canSeek: true,
      volume: 75,
      mode: 'sequential',
      modeFamily: 'ordinary',
      queueRevision: '1',
      canIntelligence: true,
      intelligence: null,
      intelligenceOperation: null,
      intelligenceRead: null,
      like: null,
      error: null,
    },
  });
  const starts: {
      ticket: string;
      command: MiniIntelligenceCommand;
      held: ReturnType<typeof deferred<MiniPlaybackReceipt>>;
    }[] = [],
    modes: { ticket: string; mode: string }[] = [],
    cancels: { epoch: string; id: string }[] = [];
  let readCount = 0;
  const receipt = (): MiniPlaybackReceipt => {
    const m = view.value!.media;
    return {
      epoch: m.epoch,
      queueRevision: m.queueRevision!,
      playId: m.selection.audioPlayId,
      mode: m.mode!,
      fm: m.modeFamily === 'fm',
      intelligence: m.intelligence,
      intelligenceOperation: m.intelligenceOperation ?? null,
    };
  };
  function update(media: Partial<MiniView['media']>) {
    view.value = {
      ...view.value!,
      ticket: String(Number(view.value!.ticket) + 1),
      media: { ...view.value!.media, ...media },
    };
  }
  const client: MiniClient = {
    read: async () => {
      readCount++;
      return view.value!;
    },
    intelligence: async (ticket, command) => {
      const held = deferred<MiniPlaybackReceipt>();
      starts.push({ ticket, command, held });
      return held.promise;
    },
    mode: async (ticket, mode) => {
      modes.push({ ticket, mode });
      update({
        mode,
        modeFamily: view.value!.media.modeFamily === 'fm' ? 'fm' : 'ordinary',
        fmModePending: null,
        intelligence: null,
        intelligenceOperation: null,
        queueRevision: String(Number(view.value!.media.queueRevision) + 1),
      });
      return receipt();
    },
    cancelIntelligence: async (epoch, id) => {
      cancels.push({ epoch, id });
      return true;
    },
    ready: async () => {},
    returnMain: async () => {},
    action: async () => {},
    queue: async () => {
      throw Error('not used');
    },
    panel: async () => {},
    preferences: async () => {},
    drag: async () => {},
    close: async () => {},
  };
  const scope = effectScope(),
    model = scope.run(() => useMiniModes(view, client, () => client.read()))!;
  return {
    view,
    client,
    model,
    starts,
    modes,
    cancels,
    scope,
    receipt,
    update,
    reads: () => readCount,
  };
}

test('mini FM uses its two-state ring and never starts intelligence or leaves its channel', async () => {
  const f = fixture();
  try {
    f.update({ modeFamily: 'fm', canIntelligence: false });
    assert.equal(f.model.mode.family.value, 'fm');
    f.model.mode.cycle();
    await turn();
    assert.equal(f.view.value!.media.mode, 'single_loop');
    assert.equal(f.model.mode.displayFamily.value, 'fm');
    const second = f.view.value!.ticket;
    f.model.mode.cycle();
    await turn();
    assert.deepEqual(
      f.modes.map((v) => v.mode),
      ['single_loop', 'sequential'],
    );
    assert.equal(f.modes[1]!.ticket, second);
    assert.equal(f.view.value!.media.modeFamily, 'fm');
    assert.equal(f.view.value!.media.selection.audioPlayId, 'old');
    assert.equal(f.starts.length, 0);
    assert.equal(f.cancels.length, 0);
  } finally {
    f.scope.stop();
  }
});

test('mini FM mode failure remains local and retries only with an explicit fresh ticket', async () => {
  const f = fixture();
  try {
    f.update({ modeFamily: 'fm', canIntelligence: false });
    const original = f.client.mode;
    let calls = 0;
    f.client.mode = async () => {
      calls++;
      throw { code: 'storage', message: 'Synthetic FM save denied', retryable: true };
    };
    f.model.mode.cycle();
    await turn();
    assert.equal(f.model.issue.value?.code, 'storage');
    assert.equal(f.model.mode.value.value, 'sequential');
    assert.equal(f.model.canRetry.value, true);
    await turn();
    assert.equal(calls, 1);
    f.update({ queueRevision: '3' });
    const ticket = f.view.value!.ticket;
    f.client.mode = original;
    f.model.retry();
    await turn();
    assert.deepEqual(f.modes, [{ ticket, mode: 'single_loop' }]);
    assert.equal(f.model.issue.value, undefined);
    assert.equal(f.view.value!.media.modeFamily, 'fm');
    assert.equal(f.starts.length, 0);
  } finally {
    f.scope.stop();
  }
});

test('reopened mini exposes an unapplied FM repeat and retries that exact target explicitly', async () => {
  const f = fixture();
  try {
    f.update({
      modeFamily: 'fm',
      canIntelligence: false,
      fmModePending: { mode: 'single_loop', revision: '7' },
    });
    assert.equal(f.model.issue.value?.code, 'fm_mode_pending');
    assert.match(f.model.issue.value!.message, /单曲循环尚未生效/);
    await turn();
    assert.equal(f.modes.length, 0);
    const ticket = f.view.value!.ticket;
    f.model.retry();
    await turn();
    assert.deepEqual(f.modes, [{ ticket, mode: 'single_loop' }]);
    assert.equal(f.model.issue.value, undefined);
    assert.equal(f.view.value!.media.fmModePending, null);
  } finally {
    f.scope.stop();
  }
});

test('FM pending dismissal survives clock reads but cannot hide another channel or a new failure', () => {
  const f = fixture();
  try {
    f.update({
      modeFamily: 'fm',
      canIntelligence: false,
      fmModePending: { mode: 'single_loop', revision: '7' },
    });
    f.model.dismiss();
    f.update({ position: 10.5 });
    assert.equal(f.model.issue.value, undefined);
    f.update({ queueRevision: '2' });
    assert.equal(f.model.issue.value?.code, 'fm_mode_pending');
    f.model.dismiss();
    f.update({ fmModePending: { mode: 'single_loop', revision: '8' } });
    assert.equal(f.model.issue.value?.code, 'fm_mode_pending');
    assert.equal(f.modes.length, 0);
  } finally {
    f.scope.stop();
  }
});

test('mini mode acknowledges its own seed while the full read remains pending, then exits with the fresh ticket', async () => {
  const f = fixture();
  try {
    f.model.mode.cycle();
    await turn();
    assert.equal(f.starts.length, 1);
    assert.equal(f.starts[0]!.ticket, '1');
    const id = f.starts[0]!.command.operation_id;
    assert.equal(f.model.preparing.value, true);
    f.update({
      queueRevision: '2',
      mode: 'order_loop',
      modeFamily: 'intelligence',
      intelligence: 'seeded',
      intelligenceOperation: id,
      intelligenceRead: { operationId: id, preparing: false },
    });
    await turn();
    assert.equal(f.model.mode.pending.value, false);
    assert.equal(f.model.busy.value, true);
    assert.equal(f.model.preparing.value, false);
    const ticket = f.view.value!.ticket;
    f.model.mode.cycle();
    await turn();
    assert.deepEqual(f.modes, [{ ticket, mode: 'order_loop' }]);
    f.update({ intelligenceRead: null });
    f.starts[0]!.held.reject({
      code: 'stale',
      message: 'Synthetic stale response',
      retryable: false,
    });
    await turn();
    assert.equal(f.model.issue.value, undefined);
    assert.equal(f.model.busy.value, false);
  } finally {
    f.scope.stop();
  }
});

test('mini footer cancellation before a seed reads back once and creates no fake mode error', async () => {
  const f = fixture();
  try {
    f.model.mode.cycle();
    await turn();
    const id = f.starts[0]!.command.operation_id;
    await f.model.cancel();
    f.starts[0]!.held.reject({
      code: 'stale',
      message: 'Synthetic stale response',
      retryable: false,
    });
    await turn();
    await turn();
    assert.deepEqual(f.cancels, [{ epoch: '1', id }]);
    assert.equal(f.model.issue.value, undefined);
    assert.equal(f.model.mode.target.value, 'sequential');
    assert.equal(f.model.mode.pending.value, false);
    assert.equal(f.modes.length, 0);
    assert.equal(f.reads(), 1);
  } finally {
    f.scope.stop();
  }
});

test('mini can cancel a main-window read and resumes only after the native pending state disappears', async () => {
  const f = fixture();
  try {
    f.update({
      mode: 'order_loop',
      modeFamily: 'intelligence',
      intelligence: 'seeded',
      intelligenceOperation: 'start',
      intelligenceRead: { operationId: 'continue', preparing: false },
    });
    assert.equal(f.model.busy.value, true);
    assert.equal(f.model.resumable.value, false);
    await f.model.cancel();
    assert.deepEqual(f.cancels, [{ epoch: '1', id: 'continue' }]);
    f.update({ intelligenceRead: null });
    await turn();
    assert.equal(f.model.resumable.value, true);
    void f.model.resume();
    await turn();
    assert.equal(f.starts[0]!.command.kind, 'continue');
    assert.equal(f.starts[0]!.ticket, f.view.value!.ticket);
    f.starts[0]!.held.resolve(f.receipt());
    await turn();
  } finally {
    f.scope.stop();
  }
});

test('mini exact mode receipt cannot carry queued clicks onto a newer source returned by read', async () => {
  const f = fixture(),
    held = deferred<MiniPlaybackReceipt>();
  try {
    f.update({ mode: 'order_loop', canIntelligence: false });
    const ticket = f.view.value!.ticket;
    f.client.mode = async (t, mode) => {
      f.modes.push({ ticket: t, mode });
      return held.promise;
    };
    f.model.mode.cycle();
    f.model.mode.cycle();
    await turn();
    f.update({
      queueRevision: '99',
      selection: { audioPlayId: 'new', audioEntryId: 'new-entry', videoPlayId: null },
      mode: 'order_loop',
    });
    held.resolve({ ...f.receipt(), queueRevision: '2', playId: 'old', mode: 'single_loop' });
    await turn();
    assert.deepEqual(f.modes, [{ ticket, mode: 'single_loop' }]);
    assert.equal(f.model.mode.target.value, 'order_loop');
    assert.equal(f.model.issue.value, undefined);
  } finally {
    f.scope.stop();
  }
});

test('mini failed preparation retries explicitly with the new ticket and a different operation UUID', async () => {
  const f = fixture();
  try {
    f.model.mode.cycle();
    await turn();
    const first = f.starts[0]!.command.operation_id;
    f.update({ queueRevision: '2' });
    f.starts[0]!.held.reject({
      code: 'storage',
      message: 'Synthetic storage denial',
      retryable: true,
    });
    await turn();
    assert.equal(f.model.issue.value?.code, 'storage');
    assert.equal(f.model.canRetry.value, true);
    const ticket = f.view.value!.ticket;
    f.model.retry();
    await turn();
    assert.equal(f.starts.length, 2);
    assert.equal(f.starts[1]!.ticket, ticket);
    assert.notEqual(f.starts[1]!.command.operation_id, first);
    f.starts[1]!.held.reject({
      code: 'stale',
      message: 'Synthetic stale response',
      retryable: false,
    });
    await turn();
  } finally {
    f.scope.stop();
  }
});

test('mini reports a main-origin fallback without replaying it, and dismisses that same persisted failure', () => {
  const f = fixture();
  try {
    f.update({
      mode: 'order_loop',
      intelligence: 'fallback',
      intelligenceOperation: 'main-origin',
    });
    assert.equal(f.model.issue.value?.code, 'intelligence_failed');
    assert.equal(f.model.canRetry.value, true);
    assert.equal(f.starts.length, 0);
    f.model.dismiss();
    f.model.retry();
    f.update({ position: 11 });
    assert.equal(f.model.issue.value, undefined);
    assert.equal(f.starts.length, 0);
  } finally {
    f.scope.stop();
  }
});
