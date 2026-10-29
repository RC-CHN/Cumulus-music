import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountView, type CoreClient } from '../services/contracts.ts';
import type {
  PlayerView,
  PlaySelection,
  PlayerAction,
  QueueView,
  LyricsView,
  LyricPreferences,
} from '../services/player.ts';
import { useCore } from '../services/core.ts';

export function usePlayer(core: CoreClient = useCore(), onSessionChanged: () => void = () => {}) {
  const view = shallowRef<PlayerView>();
  const queue = shallowRef<QueueView>();
  const lyrics = shallowRef<LyricsView>();
  const ready = ref(false),
    busy = ref(false),
    error = ref(''),
    queueError = ref(''),
    lyricError = ref('');
  const queueOpen = ref(false),
    lyricsOpen = ref(false),
    lyricLoading = ref(false);
  let epoch = '',
    connection = 0,
    request = 0,
    queueRequest = 0,
    lyricRequest = 0;
  let disconnect: (() => void) | undefined;
  let failedOperation: ((epoch: string) => Promise<PlayerView>) | undefined;
  const playing = computed(
    () => view.value?.state === 'playing' || view.value?.state === 'buffering',
  );
  const loaded = computed(() =>
    ['playing', 'paused', 'buffering'].includes(view.value?.state ?? ''),
  );
  const restorable = computed(() => !!view.value?.queueLen && view.value.state === 'idle');
  function handle(e: unknown) {
    const issue = failure(e);
    if (issue.code !== 'stale') error.value = issue.message;
    if (['session_expired', 'profile_changed'].includes(issue.code)) onSessionChanged();
  }
  function accept(next: PlayerView) {
    if (next.epoch !== epoch || (view.value && BigInt(next.revision) < BigInt(view.value.revision)))
      return;
    const old = view.value;
    view.value = next;
    if (old?.playId !== next.playId) {
      lyrics.value = undefined;
      lyricRequest++;
      if (lyricsOpen.value && next.playId) void loadLyrics();
    }
    if (queueOpen.value && old?.queueRevision !== next.queueRevision) void loadQueue();
  }
  async function connect(account: AccountView) {
    const version = ++connection;
    disconnect?.();
    disconnect = undefined;
    epoch = account.epoch;
    request++;
    failedOperation = undefined;
    queueRequest++;
    lyricRequest++;
    ready.value = false;
    busy.value = false;
    view.value = undefined;
    queue.value = undefined;
    lyrics.value = undefined;
    error.value = '';
    try {
      const stop = await core.watchPlayer(epoch, (event) => {
        if (version !== connection) return;
        if (event.kind === 'state') accept(event.value);
        else {
          ready.value = false;
          handle(event.error);
          if (event.error.code === 'stale') onSessionChanged();
        }
      });
      if (version !== connection) {
        stop();
        return;
      }
      disconnect = stop;
      ready.value = true;
      if (restorable.value)
        await run(
          (e) => core.playerAction(e, { kind: 'startup' }),
          // An explicit retry prepares paused media, never repeats automatic play.
          (e) => core.playerAction(e, { kind: 'restore' }),
        );
    } catch (e) {
      if (version === connection) handle(e);
    }
  }
  async function run(operation: (current: string) => Promise<PlayerView>, onRetry = operation) {
    if (!ready.value) return;
    const current = ++request;
    const captured = epoch;
    busy.value = true;
    error.value = '';
    try {
      const next = await operation(captured);
      if (captured === epoch) accept(next);
      if (current === request) failedOperation = undefined;
    } catch (e) {
      if (current === request && captured === epoch) {
        failedOperation = onRetry;
        handle(e);
      }
    } finally {
      if (current === request) busy.value = false;
    }
  }
  const action = (input: PlayerAction) => run((e) => core.playerAction(e, input));
  // Device preferences can be saved while a slow decoder is still loading.
  // Keep their errors and pending UI separate from playback transport requests.
  async function configureLoudness(
    input: Extract<PlayerAction, { kind: 'loudness' | 'retry_loudness' | 'repair_loudness' }>,
  ) {
    const captured = epoch;
    try {
      const next = await core.playerAction(captured, input);
      if (captured === epoch) accept(next);
    } catch (e) {
      if (captured !== epoch) return;
      if (['session_expired', 'profile_changed'].includes(failure(e).code)) onSessionChanged();
      throw e;
    }
  }
  const select = (input: PlaySelection) => run((e) => core.playSelection(e, input));
  const downloads = (ids: string[], index: number) => run((e) => core.playDownloads(e, ids, index));
  const retry = () => (failedOperation ? run(failedOperation) : action({ kind: 'refresh' }));
  async function loadQueue() {
    const current = ++queueRequest,
      captured = epoch;
    queueError.value = '';
    try {
      const value = await core.playerQueue(captured);
      if (
        current === queueRequest &&
        value.epoch === epoch &&
        value.revision === view.value?.queueRevision
      )
        queue.value = value;
    } catch (e) {
      if (current === queueRequest) queueError.value = failure(e).message;
    }
  }
  function toggleQueue() {
    queueOpen.value = !queueOpen.value;
    if (queueOpen.value) void loadQueue();
  }
  async function loadLyrics() {
    const playId = view.value?.playId;
    if (!playId) return;
    const current = ++lyricRequest,
      captured = epoch;
    lyricLoading.value = true;
    lyricError.value = '';
    try {
      const value = await core.playerLyrics(captured, playId);
      if (current === lyricRequest && value.epoch === epoch && value.playId === view.value?.playId)
        lyrics.value = value;
    } catch (e) {
      if (current === lyricRequest) lyricError.value = failure(e).message;
    } finally {
      if (current === lyricRequest) lyricLoading.value = false;
    }
  }
  function toggleLyrics() {
    lyricsOpen.value = !lyricsOpen.value;
    if (lyricsOpen.value && !lyrics.value) void loadLyrics();
  }
  async function updateLyrics(operation: (epoch: string, playId: string) => Promise<LyricsView>) {
    const playId = view.value?.playId;
    if (!playId) return;
    const current = ++lyricRequest;
    lyricError.value = '';
    try {
      const value = await operation(epoch, playId);
      if (current === lyricRequest && value.epoch === epoch && value.playId === view.value?.playId)
        lyrics.value = value;
    } catch (e) {
      if (current === lyricRequest) lyricError.value = failure(e).message;
    }
  }
  const preferences = (value: LyricPreferences) =>
    updateLyrics((e, id) => core.lyricPreferences(e, id, value));
  const offset = (gain: number) => updateLyrics((e, id) => core.lyricOffset(e, id, gain));
  function dispose() {
    connection++;
    request++;
    lyricRequest++;
    queueRequest++;
    disconnect?.();
    ready.value = false;
  }
  return {
    view,
    queue,
    lyrics,
    ready,
    busy,
    error,
    queueError,
    lyricError,
    queueOpen,
    lyricsOpen,
    lyricLoading,
    playing,
    loaded,
    connect,
    action,
    configureLoudness,
    select,
    downloads,
    retry,
    loadQueue,
    toggleQueue,
    loadLyrics,
    toggleLyrics,
    preferences,
    offset,
    dispose,
  };
}
export type PlayerModel = ReturnType<typeof usePlayer>;
