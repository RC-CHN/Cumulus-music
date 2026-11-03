import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlayer } from '../src/composables/usePlayer.ts';
import type { CoreClient, AccountView } from '../src/services/contracts.ts';
import {
  currentCatalogDownload,
  type PlayerView,
  type PlayerUpdate,
  type LyricsView,
  type PlayerClient,
  type PlayerAction,
  type PlaySelection,
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
test('the current-song download targets only the displayed catalog song', () => {
  const track = {
    id: '18',
    name: 'Song',
    artists: ['Artist'],
    album: '',
    coverUrl: null,
    durationMs: 1000,
    fee: 0,
    missing: false,
    local: false,
  };
  const view: PlayerView = { ...state('2', '4', 'audible'), resourceKind: 'catalog', track };
  assert.deepEqual(currentCatalogDownload(view), { epoch: '4', playId: 'audible', track });
  for (const resourceKind of ['local', 'downloaded', 'cloud', 'voice'] as const)
    assert.equal(currentCatalogDownload({ ...view, resourceKind }), undefined);
  assert.equal(currentCatalogDownload({ ...view, playId: null }), undefined);
  assert.equal(currentCatalogDownload({ ...view, track: { ...track, voiceId: '18' } }), undefined);
  assert.equal(currentCatalogDownload({ ...view, track: { ...track, missing: true } }), undefined);
});

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
test('album queue requests capture current queue and quality and reject repeated busy or old-account completion', async () => {
  const pending = deferred<PlayerView>(),
    calls: PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      return pending.promise;
    },
  });
  await f.player.connect(account());
  f.emit({
    ...state('2', '1', 'playing'),
    queueRevision: '42',
    qualityPreferences: {
      revision: 'saved',
      quality: 'hires',
      damaged: false,
      appliesToCurrent: true,
    },
  });
  const first = f.player.queueAlbum('7');
  await f.player.queueAlbum('8');
  assert.equal(f.player.queueBusy.value, true);
  assert.deepEqual(calls, [
    { kind: 'queue_album', id: '7', quality: 'hires', expected_queue_revision: '42' },
  ]);
  await f.player.connect(account('2'));
  pending.resolve(state('99', '1', 'old'));
  assert.equal(await first, undefined);
  assert.equal(f.player.view.value?.epoch, '2');
  assert.notEqual(f.player.view.value?.playId, 'old');
  f.player.dispose();
});
test('page-owned selection failures cannot become a footer retry with an expired snapshot', async () => {
  const calls: PlayerAction[] = [];
  let selections = 0;
  const f = fixture({
    playSelection: async () => {
      selections++;
      throw { code: 'queue_changed', message: '快照已变化' };
    },
    playerAction: async (_e, input) => {
      calls.push(input);
      return state('3');
    },
  });
  await f.player.connect(account());
  await assert.rejects(
    f.player.selectForPage({
      kind: 'cloud_batch',
      ids: ['1'],
      revision: 'old',
      quality: 'standard',
    }),
    { code: 'queue_changed' },
  );
  assert.equal(f.player.error.value, '');
  assert.equal(f.player.busy.value, false);
  await f.player.retry();
  assert.equal(selections, 1);
  assert.deepEqual(calls, []);
  f.player.dispose();
});
test('a page-owned selection reply cannot restore its previous account', async () => {
  const held = deferred<PlayerView>();
  const f = fixture({ playSelection: async () => held.promise });
  await f.player.connect(account());
  const operation = f.player.selectForPage({
    kind: 'cloud_batch',
    ids: ['1'],
    revision: 'old',
    quality: 'standard',
  });
  await f.player.connect(account('2'));
  held.resolve(state('9', '1', 'old-song'));
  assert.equal(await operation, undefined);
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.error.value, '');
  assert.equal(f.player.busy.value, false);
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
test('playlist source controls pause or resume the loaded native session without selecting it again', async () => {
  const calls: PlayerAction[] = [];
  let selections = 0;
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      return { ...state('10', '1', 'native-play'), origin: { kind: 'playlist', id: '7' } };
    },
    playSelection: async () => {
      selections++;
      return state('20');
    },
  });
  await f.player.connect(account());
  for (const status of ['playing', 'paused', 'buffering'] as const) {
    f.emit({
      ...state('11', '1', 'native-play'),
      state: status,
      origin: { kind: 'playlist', id: '7' },
      queueRevision: '9',
    });
    await f.player.togglePlaylist('7');
  }
  assert.deepEqual(
    calls,
    [true, false, true].map((paused) => ({
      kind: 'pause_for',
      play_id: 'native-play',
      expected_queue_revision: '9',
      paused,
    })),
  );
  assert.equal(selections, 0);
  f.player.dispose();
});
test('other resource origins and unloaded playlist sessions select with the saved quality', async () => {
  const selections: import('../src/services/player.ts').PlaySelection[] = [];
  let actions = 0;
  const f = fixture({
    playerAction: async () => {
      actions++;
      return state();
    },
    playSelection: async (_epoch, input) => {
      selections.push(input);
      return state('10');
    },
  });
  await f.player.connect(account());
  for (const value of [
    {
      origin: { kind: 'album' as const, id: '7' },
      state: 'playing' as const,
      playId: 'album-play',
    },
    {
      origin: { kind: 'playlist' as const, id: '8' },
      state: 'paused' as const,
      playId: 'other-play',
    },
    { origin: { kind: 'playlist' as const, id: '7' }, state: 'idle' as const, playId: null },
    {
      origin: { kind: 'playlist' as const, id: '7' },
      state: 'error' as const,
      playId: 'failed-play',
    },
  ]) {
    f.emit({
      ...state('11'),
      ...value,
      qualityPreferences: {
        revision: 'saved',
        quality: 'hires',
        damaged: false,
        appliesToCurrent: true,
      },
    });
    await f.player.togglePlaylist('7');
  }
  assert.equal(actions, 0);
  assert.equal(selections.length, 4);
  for (const input of selections)
    assert.deepEqual(input, {
      kind: 'playlist',
      id: '7',
      start_id: null,
      revision: null,
      quality: 'hires',
    });
  f.player.dispose();
});
test('a pending source command rejects repeated card clicks and cannot publish into another account', async () => {
  const result = deferred<PlayerView>();
  let actions = 0,
    selections = 0;
  const f = fixture({
    playerAction: async () => {
      actions++;
      return result.promise;
    },
    playSelection: async () => {
      selections++;
      return state();
    },
  });
  await f.player.connect(account());
  f.emit({ ...state('2', '1', 'old-play'), origin: { kind: 'playlist', id: '7' } });
  const pending = f.player.togglePlaylist('7');
  await f.player.togglePlaylist('7');
  await f.player.togglePlaylist('8');
  assert.equal(actions, 1);
  assert.equal(selections, 0);
  await f.player.connect(account('2'));
  f.emit({ ...state('2', '2', 'new-play'), origin: { kind: 'playlist', id: '8' } });
  result.resolve({ ...state('9', '1', 'old-play'), state: 'paused' });
  await pending;
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.playId, 'new-play');
  assert.equal(f.player.view.value?.state, 'playing');
  f.player.dispose();
});
test('footer transport rejects a stale rendered intent and fences the loaded ordinary track', async () => {
  const calls: PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      return state('0');
    },
  });
  await f.player.connect(account());
  f.emit({ ...state('2', '1', 'old'), queueRevision: '8' });
  const oldButton = f.player.primaryTransportKey.value;
  const playing = { ...state('3', '1', 'current'), queueRevision: '9' };
  f.emit(playing);
  await f.player.togglePrimaryTransport(oldButton);
  assert.deepEqual(calls, []);
  const currentButton = f.player.primaryTransportKey.value;
  f.emit({ ...playing, revision: '4', state: 'buffering', position: 24 });
  await f.player.togglePrimaryTransport(currentButton);
  f.emit({ ...playing, revision: '5', state: 'paused' });
  await f.player.togglePrimaryTransport(currentButton);
  await f.player.togglePrimaryTransport(f.player.primaryTransportKey.value);
  f.emit({ ...playing, revision: '6', state: 'paused', fm: true });
  await f.player.togglePrimaryTransport(f.player.primaryTransportKey.value);
  f.emit({ ...playing, revision: '7', state: 'ended' });
  await f.player.togglePrimaryTransport(f.player.primaryTransportKey.value);
  assert.deepEqual(calls, [
    { kind: 'pause_for', play_id: 'current', expected_queue_revision: '9', paused: true },
    { kind: 'pause_for', play_id: 'current', expected_queue_revision: '9', paused: false },
    { kind: 'play' },
    { kind: 'play' },
  ]);
  f.player.dispose();
});

test('shared source transport binds album, artist and voice sessions without rereading their lists', async () => {
  const calls: PlayerAction[] = [];
  let selections = 0;
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      return state('3');
    },
    playSelection: async () => {
      selections++;
      return state('3');
    },
  });
  await f.player.connect(account());
  for (const origin of [
    { kind: 'album', id: '7' },
    { kind: 'artist', id: '12' },
    { kind: 'podcast', id: '8' },
    null,
  ] as const) {
    f.emit({ ...state('4', '1', 'loaded-source'), origin, queueRevision: '9' });
    await f.player.toggleSource();
  }
  assert.equal(selections, 0);
  assert.deepEqual(
    calls,
    Array.from({ length: 4 }, () => ({
      kind: 'pause_for',
      play_id: 'loaded-source',
      expected_queue_revision: '9',
      paused: true,
    })),
  );
  f.player.dispose();
});
test('an explicit source retry retains the failed target instead of pausing another stream session', async () => {
  const calls: PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      throw { code: 'unavailable', message: '合成输出失败', retryable: true };
    },
  });
  await f.player.connect(account());
  f.emit({
    ...state('2', '1', 'paused-source'),
    state: 'paused',
    origin: { kind: 'album', id: '7' },
    queueRevision: '9',
  });
  await f.player.toggleSource();
  assert.equal(f.player.error.value, '合成输出失败');
  f.emit({
    ...state('3', '1', 'new-source'),
    origin: { kind: 'playlist', id: '8' },
    queueRevision: '10',
  });
  await f.player.retry();
  assert.deepEqual(
    calls,
    Array.from({ length: 2 }, () => ({
      kind: 'pause_for',
      play_id: 'paused-source',
      expected_queue_revision: '9',
      paused: false,
    })),
  );
  assert.equal(f.player.view.value?.playId, 'new-source');
  f.player.dispose();
});
test('source pause remains usable while a queue preparation is waiting for its read', async () => {
  const held = deferred<PlayerView>();
  const calls: PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      calls.push(input);
      return input.kind === 'queue_playlist'
        ? held.promise
        : { ...state('4', '1', 'current'), state: 'paused', origin: { kind: 'playlist', id: '7' } };
    },
  });
  await f.player.connect(account());
  f.emit({ ...state('2', '1', 'current'), origin: { kind: 'playlist', id: '7' } });
  const queue = f.player.queuePlaylist('8');
  assert.equal(f.player.queueBusy.value, true);
  await f.player.togglePlaylist('7');
  assert.equal(calls.length, 2);
  assert.deepEqual(calls[1], {
    kind: 'pause_for',
    play_id: 'current',
    expected_queue_revision: '1',
    paused: true,
  });
  assert.equal(f.player.view.value?.state, 'paused');
  held.resolve({ ...state('3', '1', 'current'), origin: { kind: 'playlist', id: '7' } });
  await queue;
  assert.equal(f.player.queueBusy.value, false);
  assert.equal(f.player.view.value?.state, 'paused');
  f.player.dispose();
});
function fixture(overrides: Partial<PlayerClient> = {}) {
  let callback: (event: PlayerUpdate) => void = () => {};
  let stopped = 0;
  const backend: PlayerClient = {
    playerSnapshot: async () => {
      throw Error('unexpected playback snapshot read');
    },
    playerIntelligence: async () => {
      throw Error('unexpected intelligence command');
    },
    playerIntelligenceCancel: async () => {
      throw Error('unexpected intelligence cancel');
    },
    playerIntelligenceEligibility: async () => {
      throw Error('unexpected intelligence eligibility');
    },
    watchPlayer: async (epoch, receive) => {
      callback = receive;
      receive({ kind: 'state', value: state('1', epoch) });
      return () => stopped++;
    },
    playSelection: async () => state(),
    preparePlaySelection: async () => 'ticket',
    commitPreparedSelection: async () => state(),
    playerAction: async () => state(),
    playerQualityDetails: async () => {
      throw Error('unexpected quality metadata read');
    },
    playerQualityTrial: async () => {
      throw Error('unexpected quality trial command');
    },
    playerQueue: async () => ({
      epoch: '1',
      revision: '1',
      fm: false,
      clearContext: { roomId: null, issue: null },
      current: null,
      tracks: [],
    }),
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
    playerQueue: async () => ({
      epoch: '0',
      revision: '1',
      fm: false,
      clearContext: { roomId: null, issue: null },
      current: null,
      tracks: [],
    }),
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
test('an account change closes old lyrics and rejects their pending result while same-account reconnect preserves the open preference', async () => {
  const held = deferred<LyricsView>();
  const f = fixture({
    playerLyrics: async (epoch, id) => (id === 'old' ? held.promise : { ...lyrics(id), epoch }),
  });
  await f.player.connect(account('1'));
  f.emit(state('2', '1', 'old'));
  f.player.toggleLyrics();
  assert.equal(f.player.lyricsOpen.value, true);
  assert.equal(f.player.lyricLoading.value, true);
  await f.player.connect(account('2'));
  held.resolve(lyrics('old'));
  await turn();
  assert.equal(f.player.lyricsOpen.value, false);
  assert.equal(f.player.lyrics.value, undefined);
  assert.equal(f.player.lyricLoading.value, false);
  assert.equal(f.player.lyricError.value, '');
  f.emit(state('2', '2', 'new'));
  f.player.toggleLyrics();
  await turn();
  assert.equal(f.player.lyrics.value?.epoch, '2');
  await f.player.connect(account('2'));
  assert.equal(f.player.lyricsOpen.value, true);
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

test('guest player connects while stale account streams still disconnect', async () => {
  let stopped = 0;
  const pending = deferred<() => void>();
  let calls = 0;
  const { player: model } = fixture({
    watchPlayer: async (epoch, receive) => {
      calls++;
      receive({ kind: 'state', value: state('1', epoch) });
      return calls === 1
        ? pending.promise
        : () => {
            stopped++;
          };
    },
  });
  const old = model.connect({ epoch: '0', status: 'unknown', nickname: null, avatarUrl: null });
  await model.connect({ epoch: '1', status: 'expired', nickname: null, avatarUrl: null });
  pending.resolve(() => {
    stopped++;
  });
  await old;
  assert.equal(stopped, 1);
  assert.equal(model.ready.value, true);
  model.dispose();
  assert.equal(stopped, 2);
});

test('startup waits for the first current snapshot if the watch acknowledgement arrives first', async () => {
  const callbacks = new Map<string, (event: PlayerUpdate) => void>();
  const calls: string[] = [];
  const f = fixture({
    watchPlayer: async (epoch, receive) => {
      callbacks.set(epoch, receive);
      return () => {};
    },
    playerAction: async (epoch, action) => {
      calls.push(`${epoch}:${action.kind}`);
      return { ...state('3', epoch, 'restored'), state: 'paused', queueLen: 2 };
    },
  });
  await f.player.connect(account('1'));
  await f.player.connect(account('2'));
  assert.equal(f.player.ready.value, false);

  callbacks.get('1')!({ kind: 'state', value: { ...state('1', '1'), queueLen: 2 } });
  callbacks.get('2')!({ kind: 'state', value: { ...state('1', '1'), queueLen: 2 } });
  assert.equal(f.player.ready.value, false);
  assert.deepEqual(calls, []);

  for (const revision of ['1', '2'])
    callbacks.get('2')!({ kind: 'state', value: { ...state(revision, '2'), queueLen: 2 } });
  await turn();
  assert.equal(f.player.ready.value, true);
  assert.deepEqual(calls, ['2:startup']);
  assert.equal(f.player.view.value?.state, 'paused');
  f.player.dispose();
});

test('a terminal stream error before acknowledgement cannot enable playback or start restoration', async () => {
  const calls: string[] = [];
  const f = fixture({
    watchPlayer: async (epoch, receive) => {
      receive({ kind: 'state', value: { ...state('1', epoch), queueLen: 2 } });
      receive({
        kind: 'error',
        error: { code: 'network', message: 'watch ended', retryable: true },
      });
      return () => {};
    },
    playerAction: async (_epoch, action) => {
      calls.push(action.kind);
      return state();
    },
  });
  await f.player.connect(account());
  assert.equal(f.player.ready.value, false);
  assert.equal(f.player.error.value, 'watch ended');
  assert.deepEqual(calls, []);
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
      if (action.kind === 'startup')
        throw { code: 'network', message: 'lost startup reply', retryable: true };
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

test('unavailable online source is actionable without replaying the rejected selection', async () => {
  let selections = 0;
  const actions: PlayerAction[] = [];
  const f = fixture({
    playSelection: async () => {
      if (++selections === 1)
        throw {
          code: 'playback_source_unavailable',
          message: '当前歌曲暂无可用音源，请选择其他歌曲',
          retryable: false,
        };
      return state('4', '1', 'other-song');
    },
    playerAction: async (_epoch, input) => {
      actions.push(input);
      return state('6', '1', 'other-song-refreshed');
    },
  });
  await f.player.connect(account());
  const selection = { kind: 'songs', ids: ['1'], index: 0, quality: 'standard' } as const;
  f.emit({ ...state('2', '1', 'old-source'), state: 'error', error: 'generic native error' });
  await f.player.select({ ...selection, ids: ['1'] });
  assert.match(f.player.error.value, /暂无可用音源/);
  assert.equal(f.player.canRetry.value, false);
  await f.player.retry();
  assert.equal(selections, 1);
  assert.deepEqual(actions, []);

  await f.player.select({ ...selection, ids: ['2'] });
  assert.equal(f.player.view.value?.playId, 'other-song');
  f.emit({ ...state('5', '1', 'other-song'), state: 'error', error: 'generic native error' });
  assert.equal(f.player.canRetry.value, true);
  await f.player.retry();
  assert.deepEqual(actions, [{ kind: 'refresh' }]);
  f.player.dispose();
});

test('quality retry refreshes the failed decoder but repeats authorization before a reload', async () => {
  for (const reload of [false, true]) {
    const actions: PlayerAction[] = [];
    const f = fixture({
      // The invoke error can reach Vue before the playback-channel event.
      playerSnapshot: async () =>
        reload
          ? {
              ...state('3', '1', 'new'),
              entryId: 'old',
              state: 'error',
              requestedQuality: 'lossless',
              error: '加载失败',
            }
          : state('2', '1', 'old'),
      playerAction: async (_epoch, input) => {
        actions.push(input);
        if (actions.length === 1) throw { code: 'playback', message: '加载失败', retryable: true };
        return state('4', '1', 'recovered');
      },
    });
    await f.player.connect(account());
    f.emit(state('2', '1', 'old'));
    const command = {
      kind: 'quality_for',
      play_id: 'old',
      expected_revision: '1',
      level: 'lossless',
    } as const;
    await f.player.action(command);
    await f.player.retry();
    assert.deepEqual(actions, [
      command,
      reload ? { kind: 'refresh_for', play_id: 'new', expected_queue_revision: '1' } : command,
    ]);
    f.player.dispose();
  }
});

test('channel-only source denial does not offer or execute a refresh', async () => {
  const actions: PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, input) => {
      actions.push(input);
      return state('4', '1', 'playing');
    },
  });
  await f.player.connect(account());
  f.emit({
    ...state('2', '1', 'denied'),
    state: 'error',
    error: '当前歌曲暂无可用音源，请选择其他歌曲',
    errorRetryable: false,
  });
  assert.equal(f.player.canRetry.value, false);
  await f.player.retry();
  assert.deepEqual(actions, []);

  f.emit({
    ...state('3', '1', 'denied'),
    state: 'error',
    error: '无法打开音频输出',
    errorRetryable: true,
  });
  assert.equal(f.player.canRetry.value, true);
  await f.player.retry();
  assert.deepEqual(actions, [{ kind: 'refresh' }]);
  f.player.dispose();
});

test('prepared card playback cannot start after an account change or newer playback request', async () => {
  let calls = 0;
  const f = fixture({
    commitPreparedSelection: async () => {
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

test('a pending card read cannot select over a newer native track or queue', async () => {
  for (const update of [
    state('3', '1', 'replacement'),
    { ...state('3', '1', 'source'), queueRevision: '2' },
  ]) {
    const pending = deferred<PlaySelection>();
    let selections = 0;
    const f = fixture({
      commitPreparedSelection: async () => {
        selections++;
        return state('4', '1', 'unexpected');
      },
    });
    await f.player.connect(account());
    f.emit(state('2', '1', 'source'));
    const reading = f.player.prepareSelection(() => pending.promise);
    await turn();
    f.emit(update);
    pending.resolve({ kind: 'songs', ids: ['1'], index: 0, quality: 'standard' });
    await reading;
    assert.equal(selections, 0);
    assert.equal(f.player.view.value?.playId, update.playId);
    assert.equal(f.player.error.value, '');
    f.player.dispose();
  }
});

test('a late native preparation ticket never starts an obsolete card read', async () => {
  const ticket = deferred<string>();
  const f = fixture({ preparePlaySelection: () => ticket.promise });
  await f.player.connect(account());
  f.emit(state('2', '1', 'source'));
  const reading = f.player.prepareSelection(async () => {
    assert.fail('obsolete preparation should not read a card');
  });
  f.emit(state('3', '1', 'replacement'));
  ticket.resolve('stale-ticket');
  await reading;
  assert.equal(f.player.view.value?.playId, 'replacement');
  assert.equal(f.player.canRetry.value, false);
  f.player.dispose();
});

test('native rejection of a prepared card before a new stream event is silent and cannot retry', async () => {
  const f = fixture({
    commitPreparedSelection: async () => {
      throw { code: 'stale', message: 'a newer native intent owns playback' };
    },
    playSelection: async () => assert.fail('prepared playback must not fall back to direct IPC'),
  });
  await f.player.connect(account());
  f.emit(state('2', '1', 'source'));
  await f.player.prepareSelection(async () => ({
    kind: 'songs',
    ids: ['1'],
    index: 0,
    quality: 'standard',
  }));
  assert.equal(f.player.view.value?.playId, 'source');
  assert.equal(f.player.error.value, '');
  assert.equal(f.player.canRetry.value, false);
  await f.player.retry();
  f.player.dispose();
});

test('a newer native track makes a failed unguarded selection and any late reply non-retryable', async () => {
  for (const late of [false, true]) {
    let reject!: (issue: unknown) => void;
    let selections = 0;
    const controls: PlayerAction[] = [];
    const f = fixture({
      playSelection: () => {
        selections++;
        return new Promise((_, fail) => (reject = fail));
      },
      playerAction: async (_epoch, command) => {
        controls.push(command);
        return state('5', '1', 'unexpected');
      },
    });
    await f.player.connect(account());
    f.emit(state('2', '1', 'source'));
    const selecting = f.player.select({ kind: 'songs', ids: ['1'], index: 0, quality: 'standard' });
    const issue = { code: 'network', message: '旧请求失败', retryable: true };
    if (!late) {
      reject(issue);
      await selecting;
      assert.equal(f.player.canRetry.value, true);
    }
    f.emit(state('3', '1', 'replacement'));
    if (late) {
      reject(issue);
      await selecting;
    }
    assert.equal(f.player.error.value, '');
    assert.equal(f.player.canRetry.value, false);
    await f.player.retry();
    assert.equal(selections, 1);
    assert.deepEqual(controls, []);
    assert.equal(f.player.view.value?.playId, 'replacement');
    f.player.dispose();
  }
});

test('retrying card preparation rereads its list before any playback command', async () => {
  let reads = 0,
    calls = 0;
  let tickets = 0;
  const f = fixture({
    preparePlaySelection: async (epoch, revision, playId) => {
      assert.deepEqual([epoch, revision, playId], ['1', '1', null]);
      return `ticket-${++tickets}`;
    },
    commitPreparedSelection: async (_epoch, ticket) => {
      assert.equal(ticket, 'ticket-2');
      calls++;
      return state('2');
    },
  });
  await f.player.connect(account());
  await f.player.prepareSelection(async () => {
    if (++reads === 1) throw { code: 'network', message: '读取失败', retryable: true };
    return { kind: 'songs', ids: ['1'], index: 0, quality: 'standard' };
  });
  assert.equal(calls, 0);
  assert.equal(f.player.error.value, '读取失败');
  await f.player.retry();
  assert.equal(reads, 2);
  assert.equal(tickets, 2);
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
  assert.deepEqual(actions, []);
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
    ['queue_for'],
  );
  assert.deepEqual(calls[0], {
    kind: 'queue_for',
    expected_queue_revision: '1',
    expected_room_id: null,
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
        ? {
            epoch: '1',
            revision: '1',
            fm: false,
            clearContext: { roomId: null, issue: null },
            current: null,
            tracks: [],
          }
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
  reading.resolve({
    epoch: '1',
    revision: '1',
    fm: false,
    clearContext: { roomId: null, issue: null },
    current: null,
    tracks: [],
  });
  assert.equal(await edit, false);
  await read;
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.queue.value, undefined);
  assert.equal(f.player.queueEditError.value, '');
  assert.equal(f.player.queueError.value, '');
  f.player.dispose();
});

test('clear consent retains its captured room and a rejection only re-reads the new policy', async () => {
  const calls: PlayerAction[] = [];
  let roomId = 'room-a',
    reads = 0;
  const f = fixture({
    playerQueue: async () => {
      reads++;
      return {
        epoch: '1',
        revision: '1',
        fm: false,
        current: null,
        tracks: [],
        clearContext: { roomId, issue: null },
      };
    },
    playerAction: async (_epoch, input) => {
      calls.push(input);
      throw { code: 'queue_changed', message: 'room changed' };
    },
  });
  await f.player.connect(account());
  await f.player.loadQueue();
  const consent = f.player.queue.value;
  roomId = 'room-b';
  await f.player.loadQueue();
  const before = reads;
  assert.equal(await f.player.editQueue({ kind: 'clear' }, consent), false);
  assert.equal(reads, before + 1);
  assert.deepEqual(calls, [
    {
      kind: 'queue_for',
      expected_queue_revision: '1',
      expected_room_id: 'room-a',
      edit: { kind: 'clear' },
    },
  ]);
  assert.equal(f.player.queue.value?.clearContext.roomId, 'room-b');
  assert.equal(f.player.queueEditError.value, 'room changed');
  assert.equal(f.player.error.value, '');
  f.player.dispose();
});

test('intelligence owns its retry separately and cannot replay an earlier failed selection', async () => {
  const held = deferred<PlayerView>();
  let selections = 0;
  const actions: PlayerAction[] = [];
  const f = fixture({
    playSelection: async () => {
      selections++;
      throw { code: 'transport', message: 'old selection failed' };
    },
    playerIntelligence: () => held.promise,
    playerAction: async (_epoch, action) => {
      actions.push(action);
      return state(action.kind === 'refresh' ? '4' : '3', '1', 'native-new');
    },
  });
  await f.player.connect(account());
  await f.player.select({ kind: 'songs', ids: ['7'], index: 0, quality: 'standard' });
  assert.equal(f.player.error.value, 'old selection failed');
  const request = f.player.intelligence({
    kind: 'start',
    operation_id: crypto.randomUUID(),
    expected_queue_revision: '1',
    origin: { kind: 'current' },
  });
  assert.equal(f.player.busy.value, false);
  assert.equal(f.player.error.value, '');
  await f.player.action({ kind: 'pause', paused: true });
  held.resolve({ ...state('2', '1', 'older-seed'), intelligence: 'active' });
  await request;
  assert.equal(f.player.view.value?.playId, 'native-new');
  await f.player.retry();
  assert.equal(selections, 1);
  assert.equal(actions.at(-1)?.kind, 'pause');
  f.player.dispose();
});

test('a late intelligence reply is rejected after reconnecting another account', async () => {
  const held = deferred<PlayerView>(),
    f = fixture({ playerIntelligence: () => held.promise });
  await f.player.connect(account());
  const request = f.player.intelligence({
    kind: 'continue',
    operation_id: crypto.randomUUID(),
    expected_queue_revision: '1',
  });
  await f.player.connect(account('2'));
  held.resolve({ ...state('100', '1', 'old-seed'), intelligence: 'active' });
  await assert.rejects(request, (e: unknown) => (e as { code: string }).code === 'stale');
  assert.equal(f.player.view.value?.epoch, '2');
  assert.equal(f.player.view.value?.playId, null);
  f.player.dispose();
});
test('local playback reconciliation keeps newer stream state and never takes transport busy or retry ownership', async () => {
  const held = deferred<PlayerView>();
  const f = fixture({ playerSnapshot: () => held.promise });
  await f.player.connect(account());
  const read = f.player.syncPlayback();
  assert.equal(f.player.busy.value, false);
  f.emit(state('8', '1', 'newest'));
  held.resolve(state('5', '1', 'older'));
  assert.equal((await read).playId, 'newest');
  assert.equal(f.player.view.value?.revision, '8');
  f.player.dispose();
});

test('a cloud toolbar preparation preserves query/quality but cannot target a newer player intent', async () => {
  const q = {
    tab: 'uploaded' as const,
    keyword: '合成',
    sort: 'title' as const,
    descending: true,
    offset: 200,
    limit: 200,
  };
  for (const changed of ['none', 'pause', 'queue', 'account']) {
    const calls: PlayerAction[] = [];
    const f = fixture({
      playerAction: async (_e, action) => {
        calls.push(action);
        return state('4', '1', 'p1');
      },
    });
    await f.player.connect(account());
    f.emit(state('2', '1', 'p1'));
    const prepared = f.player.prepareCloudQueue();
    if (changed === 'pause') await f.player.action({ kind: 'pause', paused: true });
    if (changed === 'queue') f.emit({ ...state('3', '1', 'p1'), queueRevision: 'new' });
    if (changed === 'account') await f.player.connect(account('2'));
    const before = calls.length;
    if (changed === 'none') {
      await prepared(['3', '1'], 'cloud-r1', q);
      assert.deepEqual(calls.at(-1), {
        kind: 'queue_cloud_batch',
        ids: ['3', '1'],
        revision: 'cloud-r1',
        query: q,
        quality: 'standard',
        expected_queue_revision: '1',
      });
    } else {
      assert.throws(() => prepared(['3', '1'], 'cloud-r1', q), { code: 'playback_changed' });
      assert.equal(calls.length, before);
    }
    f.player.dispose();
  }
});

test('a displayed seek cannot target a replacement song or account', async () => {
  const actions: PlayerAction[] = [];
  const f = fixture({
    playerAction: async (_epoch, action) => {
      actions.push(action);
      return state('5', '1', 'second');
    },
  });
  await f.player.connect(account());
  const first = { ...state('2', '1', 'first'), queueRevision: '6' };
  const second = { ...state('3', '1', 'second'), queueRevision: '7' };
  f.emit(first);
  f.emit(second);
  await f.player.seekDisplayed(first, 35);
  await f.player.seekDisplayed({ ...second, epoch: '0' }, 35);
  await f.player.seekDisplayed({ ...second, queueRevision: '6' }, 35);
  assert.deepEqual(actions, []);
  await f.player.seekDisplayed(second, 12);
  assert.deepEqual(actions, [
    { kind: 'seek_for', play_id: 'second', expected_queue_revision: '7', seconds: 12 },
  ]);
  f.player.dispose();
});
