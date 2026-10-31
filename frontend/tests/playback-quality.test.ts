import { test } from 'node:test';
import assert from 'node:assert/strict';
import { computed, effectScope, ref, shallowRef } from 'vue';
import { usePlaybackQuality } from '../src/composables/usePlaybackQuality.ts';
import { qualityBadge, qualityDuration, qualitySize } from '../src/services/playbackQuality.ts';
import type { PlayerModel } from '../src/composables/usePlayer.ts';
import type {
  PlayerView,
  QualityChoiceView,
  QualityDetailsView,
  QualityTrialCommand,
  QualityTrialOutcome,
  QualityTrialView,
  PlayerAction,
} from '../src/services/player.ts';
const turn = () => new Promise((resolve) => setImmediate(resolve));
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (v: unknown) => void;
  const promise = new Promise<T>((r, j) => {
    resolve = r;
    reject = j;
  });
  return { resolve, reject, promise };
}
function current(): PlayerView {
  return {
    epoch: '1',
    revision: '1',
    queueRevision: '1',
    state: 'paused',
    playId: 'song-1',
    entryId: 'entry-1',
    track: {
      id: '7',
      name: 'test',
      artists: [],
      album: '',
      coverUrl: null,
      durationMs: 120000,
      fee: 0,
      missing: false,
      local: false,
    },
    position: 10,
    timeline: null,
    fm: false,
    requestedQuality: 'standard',
    actualQuality: 'standard',
    canChangeQuality: true,
    qualityPreferences: {
      revision: 'initial',
      quality: 'standard',
      damaged: false,
      appliesToCurrent: true,
    },
    volume: 100,
    speed: 1,
    mode: 'order_loop',
    queueLen: 2,
    error: null,
    historyWarning: null,
    positionReset: false,
    equalizer: {
      revision: '0',
      settings: { enabled: false, gains: Array(10).fill(0) },
      status: 'configured',
      damaged: false,
    },
    loudness: { revision: 'initial', enabled: false, status: 'off', gainDb: null, damaged: false },
  };
}
function choice(access: QualityChoiceView['access'] = 'trial_eligible'): QualityChoiceView {
  return {
    quality: 'jymaster',
    availability: 'available',
    access,
    sizeBytes: '9007199254740993',
    trialPhase: access === 'trial_active' ? 'active' : 'eligible',
    remainingMs: 60000,
  };
}
function details(value = choice()): QualityDetailsView {
  return {
    epoch: '1',
    playId: 'song-1',
    songId: '7',
    revision: 'offer-1',
    choices: [value],
    trials: [],
    trialsDamaged: false,
    status: {
      privilege: 'ready',
      subscriber: 'ready',
      sizes: 'ready',
      useRights: { vivid: 'ready', jymaster: 'ready', sky: 'ready', jyeffect: 'ready' },
    },
    refreshAfterMs: null,
  };
}
function record(phase: QualityTrialView['phase'] = 'uncertain'): QualityTrialView {
  return {
    right: 'jymaster',
    operation: 'operation-1',
    revision: '9007199254740993',
    phase,
    acknowledged: false,
    canUse: phase === 'active',
    readFailed: false,
    remainingMs: 60000,
  };
}
function fixture() {
  const scope = effectScope();
  const view = shallowRef<PlayerView>(current());
  const actions: PlayerAction[] = [],
    commands: QualityTrialCommand[] = [];
  let read = async () => details(),
    send = async (_command: QualityTrialCommand): Promise<QualityTrialOutcome> => ({
      trial: record(),
      playback: null,
      playbackError: null,
    });
  let reads = 0;
  const player = {
    view,
    ready: ref(true),
    loaded: computed(() => ['paused', 'playing', 'buffering'].includes(view.value.state)),
    busy: ref(false),
    action: async (input: PlayerAction) => {
      actions.push(input);
      return view.value;
    },
    qualityTrial: async (epoch: string, command: QualityTrialCommand) => {
      assert.equal(epoch, view.value.epoch);
      commands.push(command);
      return send(command);
    },
  };
  const model = scope.run(() =>
    usePlaybackQuality(
      player as Pick<PlayerModel, 'view' | 'ready' | 'loaded' | 'busy' | 'action' | 'qualityTrial'>,
      {
        playerQualityDetails: async () => {
          reads++;
          return read();
        },
      },
    ),
  )!;
  return {
    scope,
    model,
    view,
    actions,
    commands,
    reads: () => reads,
    read: (value: typeof read) => {
      read = value;
    },
    send: (value: typeof send) => {
      send = value;
    },
  };
}
test('quality sizes preserve large integer precision and statuses never confuse unknown with non-membership', () => {
  assert.equal(qualitySize('9007199254740993'), '8 PB');
  assert.equal(qualitySize('123456'), '120.6 KB');
  assert.equal(qualitySize('18446744073709551615'), '16 EB');
  for (const value of ['-1', '0', 'bad', null]) assert.equal(qualitySize(value), '');
  assert.equal(qualityDuration(60001), '1 分钟 1 秒');
  assert.equal(qualityBadge(choice('unknown')), '权益待确认');
  assert.equal(qualityBadge(choice('trial_active'), record()), '待核对');
});
test('opening only reads and closing or disposing rejects late metadata without writes', async () => {
  const f = fixture(),
    pending = deferred<QualityDetailsView>();
  f.read(() => pending.promise);
  f.model.show();
  assert.equal(f.reads(), 1);
  assert.equal(f.model.loading.value, true);
  f.model.hide();
  pending.resolve(details());
  await turn();
  assert.equal(f.model.details.value, undefined);
  assert.equal(f.model.loading.value, false);
  assert.deepEqual(f.commands, []);
  assert.deepEqual(f.actions, []);
  f.scope.stop();
});
test('old-song reads cannot replace the new song even after the old response arrives last', async () => {
  const f = fixture(),
    old = deferred<QualityDetailsView>();
  f.read(() => old.promise);
  f.model.show();
  f.view.value = { ...current(), playId: 'song-2', track: { ...current().track!, id: '8' } };
  f.read(async () => ({ ...details(), playId: 'song-2', songId: '8', revision: 'offer-2' }));
  f.model.show();
  await turn();
  old.resolve(details());
  await turn();
  assert.equal(f.model.details.value?.songId, '8');
  f.scope.stop();
});
test('an offer needs confirmation and cancellation makes no activation request', async () => {
  const f = fixture();
  f.model.show();
  await turn();
  f.model.hide();
  f.model.choose('jymaster');
  assert.equal(f.model.prompt.value?.kind, 'confirm');
  assert.deepEqual(f.commands, []);
  f.model.closePrompt();
  assert.deepEqual(f.commands, []);
  f.model.show();
  await turn();
  f.model.hide();
  f.model.choose('standard');
  assert.deepEqual(f.actions, []);
  f.view.value = {
    ...current(),
    qualityPreferences: { ...current().qualityPreferences, revision: 'saved', quality: 'lossless' },
  };
  f.model.show();
  await turn();
  f.model.hide();
  f.model.choose('standard');
  assert.deepEqual(f.actions, [
    { kind: 'quality_for', play_id: 'song-1', expected_revision: 'saved', level: 'standard' },
  ]);
  f.scope.stop();
});
test('double confirmation sends once and a closed prompt cannot be reopened by its late result', async () => {
  const f = fixture(),
    pending = deferred<QualityTrialOutcome>();
  f.send(() => pending.promise);
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  const first = f.model.start();
  await f.model.start();
  assert.equal(f.commands.length, 1);
  f.model.closePrompt();
  pending.resolve({ trial: record('active'), playback: null, playbackError: null });
  await first;
  assert.equal(f.model.prompt.value, undefined);
  assert.equal(f.model.dialogBusy.value, false);
  f.scope.stop();
});
test('a lost result goes through read-only reconciliation and fresh UseActive instead of Start replay', async () => {
  const f = fixture();
  f.send(async (command) => ({
    trial: record(command.kind === 'start' ? 'uncertain' : 'active'),
    playback: command.kind === 'use_active' ? current() : null,
    playbackError: null,
  }));
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  await f.model.start();
  assert.equal(f.model.prompt.value?.kind, 'trial');
  await f.model.start();
  assert.equal(f.commands.length, 1);
  await f.model.reconcile();
  assert.equal(f.model.prompt.value?.trial?.canUse, true);
  f.read(async () => ({ ...details(choice('trial_active')), revision: 'offer-2' }));
  await f.model.useActive();
  assert.deepEqual(
    f.commands.map((c) => c.kind),
    ['start', 'reconcile', 'use_active'],
  );
  assert.deepEqual(f.commands[2], {
    kind: 'use_active',
    play_id: 'song-1',
    details_revision: 'offer-2',
    level: 'jymaster',
  });
  assert.equal(f.model.prompt.value, undefined);
  f.scope.stop();
});
test('a playback error after activation only offers reuse of the active entitlement', async () => {
  const f = fixture();
  f.send(async () => ({
    trial: { ...record('active'), acknowledged: true },
    playback: null,
    playbackError: { code: 'network', message: 'source unavailable', retryable: true },
  }));
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  await f.model.start();
  assert.equal(f.model.prompt.value?.trial?.acknowledged, true);
  assert.equal(f.model.dialogError.value, 'source unavailable');
  f.read(async () => details(choice('trial_active')));
  await f.model.useActive();
  assert.deepEqual(
    f.commands.map((c) => c.kind),
    ['start', 'use_active'],
  );
  f.scope.stop();
});
test('a changed target closes an unconfirmed offer and cannot activate it', async () => {
  const f = fixture();
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  f.view.value = { ...current(), playId: 'song-2' };
  await f.model.start();
  assert.equal(f.model.prompt.value, undefined);
  assert.deepEqual(f.commands, []);
  f.scope.stop();
});
test('a changed song may reconcile the quota result but cannot apply the old choice', async () => {
  const f = fixture(),
    pending = deferred<QualityTrialOutcome>();
  f.send(() => pending.promise);
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  const started = f.model.start();
  f.view.value = { ...current(), playId: 'song-2' };
  pending.resolve({ trial: record(), playback: null, playbackError: null });
  await started;
  assert.equal(f.model.targetChanged.value, true);
  f.send(async () => ({ trial: record('active'), playback: null, playbackError: null }));
  await f.model.reconcile();
  await f.model.useActive();
  assert.deepEqual(
    f.commands.map((c) => c.kind),
    ['start', 'reconcile'],
  );
  f.scope.stop();
});
test('switching accounts discards outstanding activation UI and never publishes its old result', async () => {
  const f = fixture(),
    pending = deferred<QualityTrialOutcome>();
  f.send(() => pending.promise);
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  const started = f.model.start();
  f.view.value = { ...current(), epoch: '2' };
  pending.resolve({ trial: record('active'), playback: null, playbackError: null });
  await started;
  assert.equal(f.model.prompt.value, undefined);
  assert.equal(f.model.details.value, undefined);
  f.scope.stop();
});
test('a thrown storage/transport error recovers the native journal without offering Start again', async () => {
  const f = fixture();
  f.send(async () => {
    throw { code: 'mutation_uncertain', message: 'please reconcile', retryable: false };
  });
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  f.read(async () => ({ ...details(), trials: [record('sending')] }));
  await f.model.start();
  assert.equal(f.model.prompt.value?.kind, 'trial');
  assert.equal(f.model.prompt.value?.trial?.phase, 'sending');
  await f.model.start();
  assert.equal(f.commands.length, 1);
  f.scope.stop();
});
test('membership notices, unknown rights, partial reads and malformed journals cannot grant a trial', async () => {
  const f = fixture();
  for (const access of ['requires_vip', 'requires_svip', 'requires_purchase', 'unknown'] as const) {
    f.read(async () => details(choice(access)));
    f.model.show();
    await turn();
    f.model.choose('jymaster');
    assert.equal(f.model.prompt.value?.kind, 'notice');
    f.model.closePrompt();
  }
  f.read(async () => ({
    ...details(),
    trialsDamaged: true,
    status: { ...details().status, sizes: 'failed' },
  }));
  f.model.show();
  await turn();
  assert.equal(f.model.partial.value, true);
  f.model.choose('jymaster');
  assert.equal(f.model.prompt.value?.kind, 'notice');
  assert.deepEqual(f.commands, []);
  assert.deepEqual(f.actions, []);
  f.scope.stop();
});
test('an existing unresolved journal takes precedence over another eligible offer and remains recoverable', async () => {
  const f = fixture();
  f.read(async () => ({ ...details(), trials: [record()] }));
  f.model.show();
  await turn();
  f.model.choose('jymaster');
  assert.equal(f.model.prompt.value?.kind, 'trial');
  assert.equal(f.model.prompt.value?.trial?.phase, 'uncertain');
  await f.model.start();
  assert.deepEqual(f.commands, []);
  f.scope.stop();
});
test('podcast choices retain their separate path and never read song usage rights', async () => {
  const f = fixture();
  f.view.value = {
    ...current(),
    qualityPreferences: { ...current().qualityPreferences, appliesToCurrent: false },
  };
  f.model.show();
  await turn();
  assert.equal(f.reads(), 0);
  f.model.choose('jymaster');
  assert.equal(f.actions[0]?.kind, 'quality_for');
  assert.deepEqual(f.commands, []);
  f.scope.stop();
});
test('rights refresh is bounded, read-only, and stops when the menu closes', async () => {
  const f = fixture();
  f.read(async () => ({ ...details(), refreshAfterMs: 1 }));
  f.model.show();
  await turn();
  assert.equal(f.reads(), 1);
  await new Promise((r) => setTimeout(r, 1050));
  assert.equal(f.reads(), 2);
  f.model.hide();
  await new Promise((r) => setTimeout(r, 1050));
  assert.equal(f.reads(), 2);
  assert.deepEqual(f.commands, []);
  f.scope.stop();
});
