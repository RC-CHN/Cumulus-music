import { test } from 'node:test';
import assert from 'node:assert/strict';
import { usePlayer } from '../src/composables/usePlayer.ts';
import type { CoreClient, AccountView } from '../src/services/contracts.ts';
import type { PlayerView, PlayerUpdate, LyricsView, PlayerClient } from '../src/services/player.ts';
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
    requestedQuality: null,
    actualQuality: null,
    volume: 100,
    speed: 1,
    mode: 'order_loop',
    queueLen: 0,
    error: null,
    positionReset: false,
    historyWarning: null,
  };
}
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
    playerQueue: async () => ({ epoch: '1', revision: '1', current: null, tracks: [] }),
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
