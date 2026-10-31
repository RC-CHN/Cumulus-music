import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlayer } from '../src/composables/usePlayer.ts';
import type { CoreClient, AccountView } from '../src/services/contracts.ts';
import type {
  PlayerView,
  PlayerUpdate,
  LyricsView,
  PlayerClient,
  PlayerAction,
} from '../src/services/player.ts';
function deferred<T>() {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}
const turn = () => new Promise((resolve) => setImmediate(resolve));
const account = (epoch = '1'): AccountView => ({
  epoch,
  status: 'authenticated',
  nickname: 'test',
  avatarUrl: null,
});
function state(revision = '1', epoch = '1', playId: string | null = null): PlayerView {
  return {
    loudness: { revision: 'initial', enabled: false, status: 'off', damaged: false, gainDb: null },
    equalizer: {
      revision: '0',
      settings: { enabled: false, gains: Array(10).fill(0) },
      status: 'configured',
      damaged: false,
    },
    epoch,
    revision,
    queueRevision: '1',
    state: playId ? 'playing' : 'idle',
    playId,
    entryId: playId,
    track: null,
    position: 0,
    timeline: null,
    fm: false,
    qualityPreferences: {
      revision: 'initial',
      quality: 'standard',
      damaged: false,
      appliesToCurrent: false,
    },
    canChangeQuality: false,
    requestedQuality: null,
    actualQuality: null,
    volume: 100,
    unmuteVolume: 10,
    speed: 1,
    mode: 'order_loop',
    queueLen: 0,
    error: null,
    positionReset: false,
    historyWarning: null,
  };
}
test('queueing a playlist uses the saved listening preference rather than current requested or actual quality', async () => {
  const calls: import('../src/services/player.ts').PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      return state('3');
    },
  });
  await f.player.connect(account());
  f.emit({
    ...state('2', '1', 'playing'),
    requestedQuality: 'exhigh',
    actualQuality: 'standard',
    qualityPreferences: {
      revision: 'saved',
      quality: 'hires',
      damaged: false,
      appliesToCurrent: true,
    },
  });
  await f.player.queuePlaylist('7');
  assert.deepEqual(calls, [
    { kind: 'queue_playlist', id: '7', quality: 'hires', expected_queue_revision: '1' },
  ]);
  f.player.dispose();
});
test('loudness preferences can be saved without completing a pending transport request', async () => {
  const loading = deferred<PlayerView>();
  const f = fixture({
    playerAction: async (_epoch, input) =>
      input.kind === 'play'
        ? loading.promise
        : {
            ...state('2'),
            loudness: {
              revision: 'saved',
              enabled: true,
              status: 'pending',
              gainDb: null,
              damaged: false,
            },
          },
  });
  await f.player.connect(account());
  const playing = f.player.action({ kind: 'play' });
  assert.equal(f.player.busy.value, true);
  await f.player.configureLoudness({
    kind: 'loudness',
    expected_revision: 'initial',
    enabled: true,
  });
  assert.equal(f.player.busy.value, true);
  assert.equal(f.player.view.value?.loudness.enabled, true);
  loading.resolve({
    ...state('3'),
    loudness: {
      revision: 'saved',
      enabled: true,
      status: 'processing',
      gainDb: -18,
      damaged: false,
    },
  });
  await playing;
  assert.equal(f.player.busy.value, false);
  assert.equal(f.player.view.value?.loudness.status, 'processing');
  f.player.dispose();
});
test('a late loudness response cannot restore the previous account state', async () => {
  const pending = deferred<PlayerView>();
  const f = fixture({ playerAction: () => pending.promise });
  await f.player.connect(account());
  const saved = f.player.configureLoudness({ kind: 'retry_loudness' });
  await f.player.connect(account('2'));
  pending.resolve({
    ...state('99', '1'),
    loudness: { revision: 'old', enabled: true, status: 'processing', gainDb: -18, damaged: false },
  });
  await saved;
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.loudness.enabled, false);
  f.player.dispose();
});
test('volume saves neither supersede pending transport nor become its error retry', async () => {
  const loading = deferred<PlayerView>();
  const calls: string[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input.kind);
      if (input.kind === 'play') return loading.promise;
      throw { code: 'storage', message: '合成音量保存失败', retryable: true };
    },
  });
  await f.player.connect(account());
  const playing = f.player.action({ kind: 'play' });
  await assert.rejects(f.player.configureVolume(0), { code: 'storage' });
  assert.equal(f.player.busy.value, true);
  assert.equal(f.player.error.value, '');
  loading.resolve(state('3'));
  await playing;
  assert.equal(f.player.busy.value, false);
  assert.deepEqual(calls, ['play', 'volume']);
  f.player.dispose();
});
test('a late volume save cannot restore state after account replacement', async () => {
  const saved = deferred<PlayerView>();
  const f = fixture({ playerAction: () => saved.promise });
  await f.player.connect(account());
  const pending = f.player.configureVolume(0);
  const rejected = assert.rejects(pending, { code: 'stale' });
  await f.player.connect(account('2'));
  saved.resolve({ ...state('99', '1'), volume: 0, unmuteVolume: 70 });
  await rejected;
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.volume, 100);
  f.player.dispose();
});
test('mode changes keep pending transport and its feedback independent', async () => {
  const loading = deferred<PlayerView>();
  const calls: import('../src/services/player.ts').PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      if (input.kind === 'play') return loading.promise;
      throw { code: 'storage', message: 'mode failure', retryable: true };
    },
  });
  await f.player.connect(account());
  const pending = f.player.action({ kind: 'play' });
  await assert.rejects(f.player.configureMode('random', '1', false), { code: 'storage' });
  assert.equal(f.player.busy.value, true);
  assert.equal(f.player.error.value, '');
  loading.resolve(state('3'));
  await pending;
  assert.deepEqual(calls, [
    { kind: 'play' },
    { kind: 'mode_for', mode: 'random', expected_queue_revision: '1', fm: false },
  ]);
  f.player.dispose();
});
test('late mode replies cannot restore a replaced account', async () => {
  const saved = deferred<PlayerView>(),
    f = fixture({ playerAction: () => saved.promise });
  await f.player.connect(account());
  const pending = f.player.configureMode('random', '1', false),
    rejected = assert.rejects(pending, { code: 'stale' });
  await f.player.connect(account('2'));
  saved.resolve({ ...state('99', '1'), mode: 'random' });
  await rejected;
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.mode, 'order_loop');
  f.player.dispose();
});
function lyrics(playId: string): LyricsView {
  return {
    epoch: '1',
    playId,
    wordLines: [],
    wordTranslation: [],
    wordRomanization: [],
    lines: [{ time: 0, lyric: playId }],
    translation: [],
    romanization: [],
    words: {},
    offset: 0,
    scrollable: true,
    displayType: 'default',
    preferences: { translationEnable: true, romaEnable: true },
    networkFailed: false,
    fromCache: false,
  };
}
function fixture(overrides: Partial<PlayerClient> = {}) {
  let callback: (event: PlayerUpdate) => void = () => {};
  let stopped = 0;
  const backend: PlayerClient = {
    watchPlayer: async (epoch, receive) => {
      callback = receive;
      receive({ kind: 'state', value: state('1', epoch) });
      return () => stopped++;
    },
    playSelection: async () => state(),
    playerAction: async () => state(),
    playerQualityDetails: async () => {
      throw Error('unexpected quality metadata read');
    },
    playerQualityTrial: async () => {
      throw Error('unexpected quality trial command');
    },
    playerQueue: async () => ({ epoch: '1', revision: '1', current: null, tracks: [] }),
    playerQueueCollection: async () => {
      throw Error('unexpected queue collection read');
    },
    playerLyrics: async (_e, id) => lyrics(id),
    lyricPreferences: async (_e, id) => lyrics(id),
    lyricOffset: async (_e, id) => lyrics(id),
    ...overrides,
  };
  return {
    player: usePlayer(backend as CoreClient),
    emit: (value: PlayerView) => callback({ kind: 'state', value }),
    stopped: () => stopped,
  };
}
test('stream revisions win over late command responses and old account events', async () => {
  const old = deferred<PlayerView>();
  const f = fixture({ playerAction: () => old.promise });
  await f.player.connect(account());
  const command = f.player.action({ kind: 'play' });
  f.emit(state('4', '1', 'new'));
  old.resolve(state('2', '1', 'old'));
  await command;
  assert.equal(f.player.view.value?.playId, 'new');
  f.emit(state('99', '0', 'other-account'));
  assert.equal(f.player.view.value?.playId, 'new');
  f.player.dispose();
});
test('a late equalizer write reply cannot replace settings after switching accounts', async () => {
  const old = deferred<PlayerView>();
  const f = fixture({ playerAction: () => old.promise });
  await f.player.connect(account('1'));
  const writing = f.player.action({
    kind: 'equalizer',
    expected_revision: '0',
    settings: { enabled: true, gains: Array(10).fill(3) },
  });
  await f.player.connect(account('2'));
  const reply = state('99', '1');
  reply.equalizer = {
    revision: '1',
    settings: { enabled: true, gains: Array(10).fill(3) },
    status: 'processing',
    damaged: false,
  };
  old.resolve(reply);
  await writing;
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.equalizer.settings.enabled, false);
  assert.equal(f.player.busy.value, false);
  f.player.dispose();
});
test('late lyrics never replace the selected song and old queues are rejected', async () => {
  const old = deferred<LyricsView>();
  const f = fixture({
    playerLyrics: async (_e, id) => (id === 'old' ? old.promise : lyrics(id)),
    playerQueue: async () => ({ epoch: '0', revision: '1', current: null, tracks: [] }),
  });
  await f.player.connect(account());
  f.emit(state('2', '1', 'old'));
  f.player.toggleLyrics();
  f.emit(state('3', '1', 'new'));
  await turn();
  old.resolve(lyrics('old'));
  await turn();
  assert.equal(f.player.lyrics.value?.playId, 'new');
  await f.player.loadQueue();
  assert.equal(f.player.queue.value, undefined);
  f.player.dispose();
});
test('subscription completing after an account switch is disposed without changing the new view', async () => {
  const old = deferred<() => void>();
  let released = 0;
  const f = fixture({
    watchPlayer: async (epoch, receive) => {
      receive({ kind: 'state', value: state('1', epoch) });
      return epoch === '1' ? old.promise : () => {};
    },
  });
  const connecting = f.player.connect(account('1'));
  await f.player.connect(account('2'));
  old.resolve(() => released++);
  await connecting;
  assert.equal(released, 1);
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.ready.value, true);
  f.player.dispose();
});
test('existing account queue requests native startup policy and signed-out state selects the separate guest queue', async () => {
  let action = '';
  const f = fixture({
    watchPlayer: async (epoch, receive) => {
      receive({ kind: 'state', value: { ...state('1', epoch), queueLen: epoch === '1' ? 5 : 0 } });
      return () => {};
    },
    playerAction: async (_epoch, a) => {
      action = a.kind;
      return { ...state('2', '1', 'restored'), state: 'paused', queueLen: 5 };
    },
  });
  await f.player.connect(account());
  assert.equal(action, 'startup');
  assert.equal(f.player.view.value?.state, 'paused');
  await f.player.connect({ ...account('2'), status: 'signed_out' });
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.queueLen, 0);
  assert.equal(f.player.view.value?.track, null);
  assert.equal(f.player.ready.value, true);
  f.player.dispose();
});

test('explicit retry after failed startup requests paused restore instead of repeating autoplay', async () => {
  const calls: string[] = [];
  const f = fixture({
    watchPlayer: async (epoch, receive) => {
      receive({ kind: 'state', value: { ...state('1', epoch), queueLen: 2 } });
      return () => {};
    },
    playerAction: async (_, action) => {
      calls.push(action.kind);
      if (action.kind === 'startup') throw { code: 'network', message: 'lost startup reply' };
      return { ...state('2', '1', 'restored'), state: 'paused', queueLen: 2 };
    },
  });
  await f.player.connect(account());
  assert.ok(f.player.error.value);
  await f.player.retry();
  assert.deepEqual(calls, ['startup', 'restore']);
  assert.equal(f.player.view.value?.state, 'paused');
  f.player.dispose();
});

test('retry repeats the failed selection rather than playing an older queue', async () => {
  let selections = 0,
    controls = 0;
  const f = fixture({
    playSelection: async () => {
      if (++selections === 1) throw { code: 'network', message: 'retry', retryable: true };
      return state('2', '1', 'selected');
    },
    playerAction: async () => {
      controls++;
      return state();
    },
  });
  await f.player.connect(account());
  await f.player.select({ kind: 'songs', ids: ['1'], index: 0, quality: 'standard' });
  await f.player.retry();
  assert.equal(selections, 2);
  assert.equal(controls, 0);
  assert.equal(f.player.view.value?.playId, 'selected');
  f.player.dispose();
});

test('prepared card playback cannot start after an account change or newer playback request', async () => {
  let calls = 0;
  const f = fixture({
    playSelection: async () => {
      calls++;
      return state('2');
    },
  });
  await f.player.connect(account());
  const old = deferred<import('../src/services/player.ts').PlaySelection>();
  const pending = f.player.prepareSelection(() => old.promise);
  assert.equal(f.player.busy.value, true);
  await f.player.connect(account('2'));
  old.resolve({ kind: 'songs', ids: ['1'], index: 0, quality: 'standard' });
  await pending;
  assert.equal(calls, 0);
  assert.equal(f.player.error.value, '');
  const replaced = deferred<import('../src/services/player.ts').PlaySelection>();
  const obsolete = f.player.prepareSelection(() => replaced.promise);
  await f.player.action({ kind: 'stop' });
  replaced.resolve({ kind: 'songs', ids: ['1'], index: 0, quality: 'standard' });
  await obsolete;
  assert.equal(calls, 0);
  f.player.dispose();
});

test('retrying card preparation rereads its list before any playback command', async () => {
  let reads = 0,
    calls = 0;
  const f = fixture({
    playSelection: async () => {
      calls++;
      return state('2');
    },
  });
  await f.player.connect(account());
  await f.player.prepareSelection(async () => {
    if (++reads === 1) throw { code: 'network', message: '读取失败' };
    return { kind: 'songs', ids: ['1'], index: 0, quality: 'standard' };
  });
  assert.equal(calls, 0);
  assert.equal(f.player.error.value, '读取失败');
  await f.player.retry();
  assert.equal(reads, 2);
  assert.equal(calls, 1);
  assert.equal(f.player.error.value, '');
  f.player.dispose();
});

test('playlist queue failures stay in their own UI and user retry takes the current fence', async () => {
  let attempts = 0;
  const f = fixture({
    playerAction: async (_e, input) => {
      assert.equal(input.kind, 'queue_playlist');
      if (input.kind !== 'queue_playlist') throw Error('wrong action');
      assert.equal(input.expected_queue_revision, attempts ? '2' : '1');
      if (!attempts++) throw { code: 'transport', message: 'playlist read failed' };
      return { ...state('3'), queueRevision: '3', queueLen: 240 };
    },
  });
  await f.player.connect(account());
  await assert.rejects(f.player.queuePlaylist('2'));
  assert.equal(f.player.error.value, '');
  assert.equal(f.player.busy.value, false);
  f.emit({ ...state('2'), queueRevision: '2' });
  assert.equal((await f.player.queuePlaylist('2'))?.queueLen, 240);
  assert.equal(attempts, 2);
  f.player.dispose();
});
test('playlist feedback must not report a superseded operation as success', async () => {
  const pending = deferred<PlayerView>();
  const f = fixture({
    playerAction: async (_e, input) =>
      input.kind === 'queue_playlist' ? pending.promise : state('3'),
  });
  await f.player.connect(account());
  const adding = f.player.queuePlaylist('2');
  assert.equal(f.player.busy.value, false);
  assert.equal(f.player.queueBusy.value, true);
  await f.player.action({ kind: 'pause', paused: true });
  pending.resolve(state('2'));
  assert.equal(await adding, undefined);
  assert.equal(f.player.queueBusy.value, false);
  assert.equal(f.player.view.value?.revision, '3');
  f.player.dispose();
});

test('a trial activation error stays outside the generic playback retry', async () => {
  let quota = 0;
  const actions: string[] = [];
  const f = fixture({
    playerQualityTrial: async () => {
      quota++;
      throw { code: 'mutation_uncertain', message: 'please reconcile', retryable: false };
    },
    playerAction: async (_e, input) => {
      actions.push(input.kind);
      return state('2');
    },
  });
  await f.player.connect(account());
  await assert.rejects(
    f.player.qualityTrial('1', {
      kind: 'start',
      play_id: 'song',
      details_revision: 'offer',
      level: 'jymaster',
    }),
  );
  assert.equal(f.player.error.value, '');
  await f.player.retry();
  assert.equal(quota, 1);
  assert.deepEqual(actions, ['refresh']);
  f.player.dispose();
});
test('a trial response after an account switch cannot replace the replacement player', async () => {
  const pending = deferred<import('../src/services/player.ts').QualityTrialOutcome>();
  const f = fixture({ playerQualityTrial: () => pending.promise });
  await f.player.connect(account());
  const old = f.player.qualityTrial('1', {
    kind: 'start',
    play_id: 'song',
    details_revision: 'offer',
    level: 'jymaster',
  });
  await f.player.connect(account('2'));
  pending.resolve({
    trial: {
      right: 'jymaster',
      operation: 'op',
      revision: '1',
      phase: 'active',
      acknowledged: true,
      canUse: true,
      readFailed: false,
      remainingMs: 60000,
    },
    playback: state('999', '1', 'old'),
    playbackError: null,
  });
  await assert.rejects(old);
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.playId, null);
  f.player.dispose();
});

test('queue edits capture the displayed revision, isolate errors and never enter audio retry', async () => {
  const calls: PlayerAction[] = [];
  let fail = true;
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      if (input.kind === 'queue_for' && fail)
        throw { code: 'queue_changed', message: 'list changed' };
      return state('3');
    },
  });
  await f.player.connect(account());
  await f.player.loadQueue();
  assert.equal(f.player.queueCurrent.value, true);
  assert.equal(await f.player.editQueue({ kind: 'clear' }), false);
  assert.equal(f.player.queueEditError.value, 'list changed');
  assert.equal(f.player.error.value, '');
  await f.player.retry();
  assert.deepEqual(
    calls.map((i) => i.kind),
    ['queue_for', 'refresh'],
  );
  assert.deepEqual(calls[0], {
    kind: 'queue_for',
    expected_queue_revision: '1',
    edit: { kind: 'clear' },
  });
  fail = false;
  assert.equal(await f.player.editQueue({ kind: 'clear' }), true);
  assert.equal(f.player.queueEditError.value, '');
  const displayed = f.player.queue.value;
  f.emit({ ...state('4'), queueRevision: '2' });
  assert.equal(f.player.queueCurrent.value, false);
  const before = calls.length;
  assert.equal(await f.player.editQueue({ kind: 'clear' }, displayed), false);
  assert.equal(calls.length, before);
  f.player.dispose();
});

test('account replacement invalidates pending queue reads and edits without leaking their errors', async () => {
  const editing = deferred<PlayerView>(),
    reading = deferred<import('../src/services/player.ts').QueueView>();
  let reads = 0;
  const f = fixture({
    playerQueue: async () =>
      ++reads === 1
        ? { epoch: '1', revision: '1', fm: false, current: null, tracks: [] }
        : reading.promise,
    playerAction: async () => editing.promise,
  });
  await f.player.connect(account());
  await f.player.loadQueue();
  const edit = f.player.editQueue({ kind: 'clear' });
  const read = f.player.loadQueue();
  assert.equal(f.player.queueEditing.value, true);
  assert.equal(f.player.queueLoading.value, true);
  await f.player.connect(account('2'));
  assert.equal(f.player.queueEditing.value, false);
  assert.equal(f.player.queueLoading.value, false);
  editing.resolve(state('99', '1'));
  reading.resolve({ epoch: '1', revision: '1', fm: false, current: null, tracks: [] });
  assert.equal(await edit, false);
  await read;
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.queue.value, undefined);
  assert.equal(f.player.queueEditError.value, '');
  assert.equal(f.player.queueError.value, '');
  f.player.dispose();
});
