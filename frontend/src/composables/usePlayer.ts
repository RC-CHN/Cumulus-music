import { computed, ref, shallowRef } from 'vue';
import { failure, type AccountView, type CoreClient } from '../services/contracts.ts';
import type {
  PlayerView,
  PlaySelection,
  PlayerAction,
  QueueView,
  LyricsView,
  LyricPreferences,
  QualityTrialCommand,
} from '../services/player.ts';
import { useCore } from '../services/core.ts';

export function usePlayer(core: CoreClient = useCore(), onSessionChanged: () => void = () => {}) {
  const view = shallowRef<PlayerView>();
  const queue = shallowRef<QueueView>();
  const lyrics = shallowRef<LyricsView>();
  const ready = ref(false),
    busy = ref(false),
    queueBusy = ref(false),
    error = ref(''),
    queueError = ref(''),
    lyricError = ref('');
  const queueOpen = ref(false),
    lyricsOpen = ref(false),
    lyricLoading = ref(false);
  let epoch = '',
    connection = 0,
    request = 0,
    queuePreparation = 0,
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
    queueBusy.value = false;
    queuePreparation++;
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
      if (captured === epoch && current === request) return next;
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
  // Quota writes must never become the generic audio-error retry operation.
  // A closed trial dialog may leave a native operation running; its durable
  // result is still accepted for this connection, independently of that dialog.
  async function qualityTrial(expectedEpoch: string, command: QualityTrialCommand) {
    const owner = epoch,
      session = connection,
      control = request;
    if (!ready.value || owner !== expectedEpoch)
      throw { code: 'stale', message: '账号或播放状态已变化，请重新选择音质', retryable: false };
    try {
      const result = await core.playerQualityTrial(owner, command);
      if (owner !== epoch || session !== connection || !ready.value)
        throw { code: 'stale', message: '账号已切换，请重新选择音质', retryable: false };
      if (result.playback) {
        accept(result.playback);
        if (control === request) {
          failedOperation = undefined;
          error.value = '';
        }
      }
      return result;
    } catch (e) {
      if (
        owner === epoch &&
        session === connection &&
        ['session_expired', 'profile_changed'].includes(failure(e).code)
      )
        onSessionChanged();
      throw e;
    }
  }
  // Resource-menu failures belong to that operation, not the audio-error bar.
  // A user retry captures the latest queue fence; no automatic write replay.
  function queuePlaylist(id: string, selection?: { ids: string[]; revision: string }) {
    const quality = view.value?.qualityPreferences.quality ?? 'standard';
    return queueAction(
      selection
        ? { kind: 'queue_playlist_batch', id, quality, ...selection }
        : { kind: 'queue_playlist', id, quality },
    );
  }
  function queueDownloads(task_ids: string[]) {
    return queueAction({ kind: 'queue_downloads', task_ids });
  }
  type QueueInput<T> = T extends { expected_queue_revision: string }
    ? Omit<T, 'expected_queue_revision'>
    : never;
  async function queueAction(
    action: QueueInput<
      Extract<PlayerAction, { kind: 'queue_downloads' | 'queue_playlist' | 'queue_playlist_batch' }>
    >,
  ) {
    if (!ready.value || busy.value || queueBusy.value) return;
    const captured = epoch,
      current = ++request,
      preparation = ++queuePreparation;
    queueBusy.value = true;
    try {
      const input: PlayerAction = {
        ...action,
        expected_queue_revision: view.value?.queueRevision ?? '0',
      };
      const next = await core.playerAction(captured, input);
      if (captured === epoch) accept(next);
      if (current === request && captured === epoch) return next;
    } catch (issue) {
      if (current === request && captured === epoch) throw issue;
    } finally {
      if (preparation === queuePreparation) queueBusy.value = false;
    }
  }
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
  // Volume uses absolute, repeatable values and its own feedback. It must not
  // supersede a pending transport request or become the audio-error retry.
  async function configureVolume(value: number) {
    const captured = epoch,
      session = connection;
    if (!ready.value) throw { code: 'stale', message: '播放器尚未连接', retryable: false };
    try {
      const next = await core.playerAction(captured, { kind: 'volume', value });
      if (captured !== epoch || session !== connection || !ready.value)
        throw { code: 'stale', message: '账号或播放器连接已变化', retryable: false };
      accept(next);
      return next;
    } catch (e) {
      if (
        captured === epoch &&
        session === connection &&
        ['session_expired', 'profile_changed'].includes(failure(e).code)
      )
        onSessionChanged();
      throw e;
    }
  }
  async function configureMode(
    mode: import('../services/player').PlaybackMode,
    queueRevision: string,
    fm: boolean,
  ) {
    const captured = epoch,
      session = connection;
    if (!ready.value) throw { code: 'stale', message: '播放器尚未连接', retryable: false };
    try {
      const next = await core.playerAction(captured, {
        kind: 'mode_for',
        mode,
        expected_queue_revision: queueRevision,
        fm,
      });
      if (captured !== epoch || session !== connection || !ready.value)
        throw { code: 'stale', message: '账号或播放器连接已变化', retryable: false };
      accept(next);
      return next;
    } catch (e) {
      if (
        captured === epoch &&
        session === connection &&
        ['session_expired', 'profile_changed'].includes(failure(e).code)
      )
        onSessionChanged();
      throw e;
    }
  }
  const select = (input: PlaySelection) => run((e) => core.playSelection(e, input));
  // Card playback reads its authoritative list before selecting it. A delayed
  // read must not start audio after account replacement or a newer operation.
  const prepareSelection = (prepare: (epoch: string) => Promise<PlaySelection>) =>
    run(async (e) => {
      const captured = request;
      const input = await prepare(e);
      if (captured !== request || e !== epoch) throw { code: 'stale', message: '播放请求已更新' };
      return core.playSelection(e, input);
    });
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
    queuePreparation++;
    queueBusy.value = false;
    disconnect?.();
    ready.value = false;
  }
  return {
    queuePlaylist,
    queueDownloads,
    prepareSelection,
    view,
    queue,
    lyrics,
    ready,
    busy,
    queueBusy,
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
    qualityTrial,
    configureLoudness,
    configureVolume,
    configureMode,
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
