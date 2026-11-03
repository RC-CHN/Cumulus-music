import { computed, ref, shallowRef } from 'vue';
import {
  failure,
  type AccountView,
  type CoreClient,
  type CoreFailure,
} from '../services/contracts.ts';
import type {
  PlayerView,
  PlaySelection,
  PlayerAction,
  QueueView,
  QueueEdit,
  LyricsView,
  LyricPreferences,
  QualityTrialCommand,
  IntelligenceCommand,
} from '../services/player.ts';
import { useCore } from '../services/core.ts';
import type { CatalogSelectionPage } from '../services/catalog.ts';
import type { CloudQuery } from '../services/cloud.ts';
import type { LocalQuery } from '../services/local.ts';
import { useEqualizerDraft } from './useEqualizerDraft.ts';

function sameSelection(a: PlayerView | undefined, b: PlayerView | undefined) {
  return (
    a?.playId === b?.playId && a?.entryId === b?.entryId && a?.queueRevision === b?.queueRevision
  );
}

export function usePlayer(core: CoreClient = useCore(), onSessionChanged: () => void = () => {}) {
  const view = shallowRef<PlayerView>();
  const queue = shallowRef<QueueView>();
  const lyrics = shallowRef<LyricsView>();
  const ready = ref(false),
    busy = ref(false),
    queueBusy = ref(false),
    queueLoading = ref(false),
    queueEditing = ref(false),
    queueError = ref(''),
    queueEditError = ref(''),
    lyricError = ref('');
  const reported = shallowRef<CoreFailure>();
  const error = computed(() => reported.value?.message ?? '');
  const canRetry = computed(
    () => reported.value?.retryable ?? view.value?.errorRetryable ?? !!view.value?.error,
  );
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
  type PlaybackOperation = (epoch: string) => Promise<PlayerView>;
  let failedOperation: { repeat?: PlaybackOperation; selection?: PlayerView } | undefined;
  const playing = computed(
    () => view.value?.state === 'playing' || view.value?.state === 'buffering',
  );
  const loaded = computed(() =>
    ['playing', 'paused', 'buffering'].includes(view.value?.state ?? ''),
  );
  // Bind the rendered footer action to its selection, without letting
  // harmless clock/buffer updates invalidate the button.
  const primaryTransportKey = computed(() => {
    const current = view.value;
    return JSON.stringify([
      current?.epoch,
      current?.playId,
      current?.queueRevision,
      current?.fm,
      playing.value ? 'pause' : current?.state,
    ]);
  });
  const restorable = computed(
    () => !!(view.value?.queueLen || view.value?.track) && view.value?.state === 'idle',
  );
  const queueCurrent = computed(
    () =>
      ready.value &&
      !!queue.value &&
      queue.value.epoch === view.value?.epoch &&
      queue.value.revision === view.value.queueRevision,
  );
  function handle(e: unknown, present = true) {
    const issue = failure(e);
    if (present && issue.code !== 'stale') reported.value = issue;
    if (['session_expired', 'profile_changed'].includes(issue.code)) onSessionChanged();
    return issue;
  }
  function accept(next: PlayerView) {
    if (next.epoch !== epoch || (view.value && BigInt(next.revision) < BigInt(view.value.revision)))
      return;
    const old = view.value;
    view.value = next;
    // A native shortcut, mini player or queue step can select a replacement
    // without touching this renderer's request counter.
    if (failedOperation?.selection && !sameSelection(failedOperation.selection, next)) {
      failedOperation = undefined;
      reported.value = undefined;
    }
    if (old?.playId !== next.playId) {
      lyrics.value = undefined;
      lyricRequest++;
      if (lyricsOpen.value && next.playId) void loadLyrics();
    }
    if (queueOpen.value && old?.queueRevision !== next.queueRevision) void loadQueue();
  }
  async function connect(account: AccountView) {
    if (epoch !== account.epoch) lyricsOpen.value = false;
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
    queueLoading.value = false;
    queueEditing.value = false;
    queueError.value = '';
    queueEditError.value = '';
    queuePreparation++;
    view.value = undefined;
    queue.value = undefined;
    lyrics.value = undefined;
    lyricLoading.value = false;
    lyricError.value = '';
    reported.value = undefined;
    let registered = false,
      ended = false;
    function finish() {
      // The channel's initial state and the invoke reply can arrive in
      // either order. Don't miss startup or re-run it on later snapshots.
      if (version !== connection || !registered || ended || !view.value || ready.value) return;
      ready.value = true;
      if (restorable.value)
        return run(
          (e) => core.playerAction(e, { kind: 'startup' }),
          // An explicit retry prepares paused media, never repeats automatic play.
          (e) => core.playerAction(e, { kind: 'restore' }),
        );
    }
    try {
      const stop = await core.watchPlayer(epoch, (event) => {
        if (version !== connection || ended) return;
        if (event.kind === 'state') {
          accept(event.value);
          void finish();
        } else {
          ended = true;
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
      registered = true;
      await finish();
    } catch (e) {
      if (version === connection) handle(e);
    }
  }
  async function run(operation: PlaybackOperation, onRetry = operation, selectionScoped = false) {
    if (!ready.value) return;
    const current = ++request;
    const captured = epoch;
    const origin = selectionScoped ? view.value : undefined;
    busy.value = true;
    reported.value = undefined;
    try {
      const next = await operation(captured);
      if (captured === epoch) accept(next);
      if (current === request) failedOperation = undefined;
      if (captured === epoch && current === request) return next;
    } catch (e) {
      if (current === request && captured === epoch) {
        const currentSelection = !selectionScoped || sameSelection(origin, view.value);
        const issue = handle(e, currentSelection);
        failedOperation =
          currentSelection && issue.code !== 'stale'
            ? { repeat: issue.retryable ? onRetry : undefined, selection: origin }
            : undefined;
      }
    } finally {
      if (current === request) busy.value = false;
    }
  }
  function action(input: PlayerAction) {
    const invoke = (e: string) => core.playerAction(e, input);
    if (input.kind !== 'quality_for') return run(invoke);
    const origin = view.value;
    return run(invoke, async (e) => {
      const intent = request;
      await syncPlayback();
      if (e !== epoch || intent !== request)
        throw { code: 'stale', message: '播放请求已更新', retryable: false };
      const current = view.value;
      // Authorization errors leave the original player intact. If a newly
      // authorized decoder failed, its entry already owns the chosen quality.
      const failedReload =
        origin?.entryId &&
        current?.epoch === e &&
        current.entryId === origin.entryId &&
        current.playId &&
        current.playId !== origin.playId &&
        current.state === 'error' &&
        current.requestedQuality === input.level;
      return failedReload
        ? core.playerAction(e, {
            kind: 'refresh_for',
            play_id: current.playId!,
            expected_queue_revision: current.queueRevision,
          })
        : invoke(e);
    });
  }
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
          reported.value = undefined;
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
  function queueLocal(query: LocalQuery, revision: string) {
    return queueAction({ kind: 'queue_local', query, revision });
  }
  function queueCloud(ids: string[], revision: string, query?: CloudQuery) {
    return queueAction({
      kind: 'queue_cloud_batch',
      ...(query ? { query } : {}),
      ids,
      revision,
      quality: view.value?.qualityPreferences.quality ?? 'standard',
    });
  }
  function queueArtistSongs(pages: CatalogSelectionPage[]) {
    return queueAction({
      kind: 'queue_artist_songs',
      pages,
      quality: view.value?.qualityPreferences.quality ?? 'standard',
    });
  }
  function queueDaily(revision: string, indices?: number[]) {
    return queueAction({
      kind: 'queue_daily_recommendations',
      revision,
      indices,
      quality: view.value?.qualityPreferences.quality ?? 'standard',
    });
  }
  function queueStyleRecommendations(revision: string) {
    return queueAction({
      kind: 'queue_style_recommendations',
      revision,
      quality: view.value?.qualityPreferences.quality ?? 'standard',
    });
  }
  function queueAlbum(id: string, selection?: { ids: string[]; revision: string }) {
    const quality = view.value?.qualityPreferences.quality ?? 'standard';
    return queueAction(
      selection
        ? { kind: 'queue_album_batch', id, quality, ...selection }
        : { kind: 'queue_album', id, quality },
    );
  }
  // A toolbar may first read multiple catalog pages. Capture the user's player
  // intent before that read, so its late result cannot target a newer queue.
  function prepareCloudQueue() {
    const captured = epoch,
      session = connection,
      intent = request,
      before = view.value;
    return (ids: string[], revision: string, query: CloudQuery) => {
      const current = view.value;
      if (
        !ready.value ||
        captured !== epoch ||
        session !== connection ||
        intent !== request ||
        !before ||
        !current ||
        before.queueRevision !== current.queueRevision ||
        before.playId !== current.playId ||
        before.entryId !== current.entryId ||
        before.state !== current.state ||
        before.fm !== current.fm
      )
        throw { code: 'playback_changed', message: '播放状态已变化，请重新执行添加。' };
      return queueCloud(ids, revision, query);
    };
  }
  function queueDownloads(task_ids: string[]) {
    return queueAction({ kind: 'queue_downloads', task_ids });
  }
  type QueueInput<T> = T extends { expected_queue_revision: string }
    ? Omit<T, 'expected_queue_revision'>
    : never;
  async function queueAction(
    action: QueueInput<
      Extract<
        PlayerAction,
        {
          kind:
            | 'queue_downloads'
            | 'queue_local'
            | 'queue_playlist'
            | 'queue_daily_recommendations'
            | 'queue_style_recommendations'
            | 'queue_playlist_batch'
            | 'queue_cloud_batch'
            | 'queue_album_batch'
            | 'queue_album'
            | 'queue_artist_songs';
        }
      >
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
  async function configureEqualizer(
    input: Extract<
      PlayerAction,
      { kind: 'equalizer' | 'save_custom_equalizer' | 'retry_equalizer' | 'repair_equalizer' }
    >,
  ) {
    const captured = epoch,
      session = connection;
    if (!ready.value) throw { code: 'stale', message: '播放器尚未连接', retryable: false };
    try {
      const next = await core.playerAction(captured, input);
      if (captured !== epoch || session !== connection || !ready.value)
        throw { code: 'stale', message: '账号或播放器连接已变化', retryable: false };
      accept(next);
      return next;
    } catch (issue) {
      if (
        captured === epoch &&
        session === connection &&
        ['session_expired', 'profile_changed'].includes(failure(issue).code)
      )
        onSessionChanged();
      throw issue;
    }
  }
  // The draft belongs to the player connection, so closing the EQ dialog
  // cannot discard its last coalesced slider position.
  const equalizerDraft = useEqualizerDraft(() => view.value, configureEqualizer, syncPlayback);
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
  async function syncPlayback() {
    const captured = epoch,
      session = connection;
    if (!ready.value) throw { code: 'stale', message: '播放器尚未连接', retryable: false };
    const next = await core.playerSnapshot(captured);
    if (captured !== epoch || session !== connection || !ready.value)
      throw { code: 'stale', message: '账号或播放器连接已变化', retryable: false };
    accept(next);
    return view.value!;
  }
  async function intelligence(command: IntelligenceCommand) {
    const captured = epoch,
      session = connection;
    if (!ready.value) throw { code: 'stale', message: '播放器尚未连接', retryable: false };
    // This explicit selection supersedes an older generic playback retry, but
    // recommendation I/O must leave transport controls usable.
    request++;
    busy.value = false;
    failedOperation = undefined;
    reported.value = undefined;
    try {
      const next = await core.playerIntelligence(captured, command);
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
  const runSelection = (operation: PlaybackOperation) => run(operation, operation, true);
  const select = (input: PlaySelection) => runSelection((e) => core.playSelection(e, input));
  function seekDisplayed(
    target: Pick<PlayerView, 'epoch' | 'playId' | 'queueRevision'>,
    seconds: number,
  ) {
    const current = view.value;
    if (
      !ready.value ||
      busy.value ||
      !loaded.value ||
      !target.playId ||
      target.epoch !== current?.epoch ||
      target.playId !== current.playId ||
      target.queueRevision !== current.queueRevision
    )
      return;
    return action({
      kind: 'seek_for',
      play_id: target.playId,
      expected_queue_revision: target.queueRevision,
      seconds,
    });
  }
  function togglePrimaryTransport(displayedKey: string | undefined) {
    const current = view.value;
    if (
      !current ||
      !ready.value ||
      busy.value ||
      displayedKey !== primaryTransportKey.value ||
      current.state === 'preparing' ||
      current.state === 'loading'
    )
      return;
    if (loaded.value && !current.fm) return toggleSource();
    // An inactive selection needs a new load; FM play uses its coordinator.
    return action(playing.value ? { kind: 'pause', paused: true } : { kind: 'play' });
  }
  function toggleSource() {
    if (!ready.value || busy.value) return;
    const current = view.value;
    if (!current?.playId || !loaded.value) return;
    return action({
      kind: 'pause_for',
      play_id: current.playId,
      expected_queue_revision: current.queueRevision,
      paused: playing.value,
    });
  }
  function togglePlaylist(id: string) {
    if (!ready.value || busy.value) return;
    const current = view.value;
    if (
      current?.origin?.kind === 'playlist' &&
      current.origin.id === id &&
      current.playId &&
      !current.fm &&
      loaded.value
    )
      return toggleSource();
    return select({
      kind: 'playlist',
      id,
      start_id: null,
      revision: null,
      quality: current?.qualityPreferences.quality ?? 'standard',
    });
  }
  // A page with its own snapshot/retry UI owns this failure. Do not retain a
  // footer retry that would resend its captured, potentially expired revision.
  async function selectForPage(input: PlaySelection) {
    if (!ready.value || busy.value || queueBusy.value) return;
    const captured = epoch,
      session = connection,
      current = ++request;
    busy.value = true;
    reported.value = undefined;
    failedOperation = undefined;
    try {
      const next = await core.playSelection(captured, input);
      if (captured === epoch && session === connection) accept(next);
      if (captured === epoch && session === connection && current === request) return next;
    } catch (issue) {
      if (captured === epoch && session === connection && current === request) throw issue;
    } finally {
      if (current === request) busy.value = false;
    }
  }
  // Acquire native ownership before reading the card's list. The renderer check
  // avoids redundant reads; native also sees newer intent before events arrive.
  const prepareSelection = (prepare: (epoch: string) => Promise<PlaySelection>) =>
    runSelection(async (e) => {
      const captured = request,
        origin = view.value;
      const requireCurrent = () => {
        if (captured !== request || e !== epoch || !sameSelection(origin, view.value))
          throw { code: 'stale', message: '播放请求已更新' };
      };
      if (!origin || origin.epoch !== e) throw { code: 'stale', message: '播放状态尚未就绪' };
      const ticket = await core.preparePlaySelection(e, origin.queueRevision, origin.playId);
      requireCurrent();
      const input = await prepare(e);
      requireCurrent();
      return core.commitPreparedSelection(e, ticket, input);
    });
  const downloads = (ids: string[], index: number) =>
    runSelection((e) => core.playDownloads(e, ids, index));
  function retry() {
    if (!canRetry.value) return;
    const failed = failedOperation;
    return failed?.repeat
      ? run(failed.repeat, failed.repeat, !!failed.selection)
      : action({ kind: 'refresh' });
  }
  async function loadQueue() {
    const current = ++queueRequest,
      captured = epoch;
    queueError.value = '';
    queueLoading.value = true;
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
    } finally {
      if (current === queueRequest) queueLoading.value = false;
    }
  }
  // Queue edits belong to the list the user saw, with their own pending/error
  // feedback. They must never become the footer's audio-error retry operation.
  async function editQueue(input: QueueEdit, displayed = queue.value): Promise<boolean> {
    if (!ready.value || busy.value || queueBusy.value || queueEditing.value) return false;
    if (
      !displayed ||
      displayed.epoch !== epoch ||
      displayed.revision !== view.value?.queueRevision
    ) {
      queueEditError.value = '播放列表已更新，请核对当前列表后重试';
      return false;
    }
    const captured = epoch,
      session = connection;
    queueEditing.value = true;
    queueEditError.value = '';
    try {
      const next = await core.playerAction(captured, {
        kind: 'queue_for',
        expected_queue_revision: displayed.revision,
        ...(input.kind === 'clear' ? { expected_room_id: displayed.clearContext.roomId } : {}),
        edit: input,
      });
      if (session !== connection || captured !== epoch || !ready.value) return false;
      accept(next);
      return true;
    } catch (issue) {
      if (session === connection && captured === epoch) {
        const problem = failure(issue);
        queueEditError.value = problem.message;
        if (['session_expired', 'profile_changed'].includes(problem.code)) onSessionChanged();
        // Re-read room policy after a rejected confirmation. This never
        // retries the clear or adopts a new room as the old confirmation.
        if (input.kind === 'clear') await loadQueue();
      }
      return false;
    } finally {
      if (session === connection && captured === epoch) queueEditing.value = false;
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
    equalizerDraft.reset();
    connection++;
    request++;
    lyricRequest++;
    queueRequest++;
    queuePreparation++;
    queueBusy.value = false;
    queueLoading.value = false;
    queueEditing.value = false;
    disconnect?.();
    ready.value = false;
  }
  return {
    queuePlaylist,
    queueLocal,
    queueAlbum,
    queueDaily,
    queueStyleRecommendations,
    queueArtistSongs,
    queueDownloads,
    queueCloud,
    prepareCloudQueue,
    prepareSelection,
    view,
    queue,
    lyrics,
    ready,
    busy,
    queueBusy,
    queueLoading,
    queueEditing,
    queueCurrent,
    error,
    canRetry,
    queueError,
    queueEditError,
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
    configureEqualizer,
    equalizerDraft,
    configureVolume,
    configureMode,
    intelligence,
    syncPlayback,
    select,
    toggleSource,
    primaryTransportKey,
    togglePrimaryTransport,
    seekDisplayed,
    togglePlaylist,
    selectForPage,
    downloads,
    retry,
    loadQueue,
    editQueue,
    toggleQueue,
    loadLyrics,
    toggleLyrics,
    preferences,
    offset,
    dispose,
  };
}
export type PlayerModel = ReturnType<typeof usePlayer>;
