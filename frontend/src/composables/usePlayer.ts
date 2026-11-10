import { computed, ref, shallowRef } from 'vue';
import {
  failure,
  type AccountView,
  type CoreClient,
  type CoreFailure,
} from '../services/contracts.ts';
import type {
  PlayerView,
  PlayerUpdate,
  PlaySelection,
  PlayerAction,
  QueueView,
  QueueEdit,
  QueueDropSource,
  LyricsView,
  LyricPreferences,
  Quality,
  QualityTrialCommand,
  IntelligenceCommand,
} from '../services/player.ts';
import { queueMoreRecommendations } from '../services/player.ts';
import { useCore } from '../services/core.ts';
import type { CatalogSelectionPage } from '../services/catalog.ts';
import type { CloudQuery } from '../services/cloud.ts';
import type { LocalQuery } from '../services/local.ts';
import { useEqualizerDraft } from './useEqualizerDraft.ts';
import type { VinylSimilarAction, VinylSimilarSeed } from '../services/vinylSimilar.ts';
import type {
  VoiceSongAction,
  VoiceSongSelectionAction,
  VoiceSongsScope,
} from '../services/voiceSongs.ts';

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
    queueMoreChanging = ref(false),
    queueMoreError = ref(''),
    lyricError = ref('');
  const pendingSourceCommand = shallowRef<{ source: PlayerView }>();
  const loadingStepCommand = shallowRef<{ key: string }>();
  const reported = shallowRef<CoreFailure>();
  const reportedRevision = ref(0);
  const ordinarySkipNotices = shallowRef<string[]>([]);
  const skipNoticeTimers = new Map<string, ReturnType<typeof setTimeout>>();
  function clearSkipNotices() {
    for (const timer of skipNoticeTimers.values()) clearTimeout(timer);
    skipNoticeTimers.clear();
    ordinarySkipNotices.value = [];
  }
  function showSkipNotice(event: Extract<PlayerUpdate, { kind: 'ordinary_copyright_skip' }>) {
    const { id, remainingMs } = event;
    if (
      event.epoch !== epoch ||
      !/^[0-9a-f]{32}$/.test(id) ||
      !Number.isInteger(remainingMs) ||
      remainingMs < 1 ||
      remainingMs > 3000 ||
      skipNoticeTimers.has(id)
    )
      return;
    function remove(key: string) {
      clearTimeout(skipNoticeTimers.get(key));
      skipNoticeTimers.delete(key);
      ordinarySkipNotices.value = ordinarySkipNotices.value.filter((old) => old !== key);
    }
    if (ordinarySkipNotices.value.length >= 16) remove(ordinarySkipNotices.value[0]!);
    ordinarySkipNotices.value = [...ordinarySkipNotices.value, id];
    skipNoticeTimers.set(
      id,
      setTimeout(() => remove(id), remainingMs),
    );
  }
  const error = computed(() => reported.value?.message ?? '');
  const errorCode = computed(() => reported.value?.code ?? view.value?.errorCode);
  const canPlayMissingDownloadOnline = computed(() => {
    const current = view.value;
    return (
      ready.value &&
      current?.epoch === epoch &&
      current.state === 'error' &&
      !!current.playId &&
      !!current.entryId &&
      current.errorCode === 'playback_download_missing'
    );
  });
  const canRetry = computed(
    () =>
      !canPlayMissingDownloadOnline.value &&
      (reported.value?.retryable ?? view.value?.errorRetryable ?? !!view.value?.error),
  );
  const queueOpen = ref(false),
    lyricsOpen = ref(false),
    lyricLoading = ref(false),
    lyricUpdating = ref(false);
  let epoch = '',
    connection = 0,
    request = 0,
    queuePreparation = 0,
    queueRequest = 0,
    lyricRequest = 0;
  let lyricProjectionPending = false;
  let lyricProjectionRequest: symbol | undefined;
  let disconnect: (() => void) | undefined;
  type PlaybackOperation = (epoch: string) => Promise<PlayerView>;
  let failedOperation: { repeat?: PlaybackOperation; selection?: PlayerView } | undefined;
  const playing = computed(
    () => view.value?.state === 'playing' || view.value?.state === 'buffering',
  );
  const loaded = computed(() =>
    ['playing', 'paused', 'buffering'].includes(view.value?.state ?? ''),
  );
  const pendingTransportBusy = computed(() => {
    const source = pendingSourceCommand.value?.source;
    return !!source && source.epoch === view.value?.epoch && sameSelection(source, view.value);
  });
  const pendingTransport = computed(() => {
    const current = view.value;
    if (
      !current?.playId ||
      current.fm ||
      (current.state !== 'preparing' && current.state !== 'loading') ||
      current.pendingPaused === undefined
    )
      return;
    return {
      playId: current.playId,
      revision: current.queueRevision,
      paused: current.pendingPaused,
    };
  });
  const primaryTransportPauses = computed(
    () => playing.value || pendingTransport.value?.paused === false,
  );
  const stepIdentity = (current: PlayerView) =>
    JSON.stringify([current.epoch, current.playId, current.queueRevision]);
  const loadingStepKey = computed(() => {
    const current = view.value;
    if (current && pendingTransport.value && current.queueLen > 1 && !current.intelligence)
      return stepIdentity(current);
  });
  const endedStepKey = computed(() => {
    const current = view.value;
    if (
      current?.state === 'ended' &&
      !current.playId &&
      current.endedMinibarReceipt &&
      current.entryId &&
      !current.fm &&
      !current.intelligence
    ) {
      return JSON.stringify([
        current.epoch,
        current.endedMinibarReceipt,
        current.entryId,
        current.queueRevision,
      ]);
    }
  });
  const loadingStepBusy = computed(
    () => !!loadingStepCommand.value && loadingStepCommand.value.key === loadingStepKey.value,
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
      pendingTransport.value
        ? pendingTransport.value.paused
          ? 'pending-play'
          : 'pending-pause'
        : playing.value
          ? 'pause'
          : current?.state,
    ]);
  });
  // A footer click must match the failure the button actually displays.
  // Playback ticks do not change this key.
  const retryKey = computed(() => {
    const current = view.value;
    return JSON.stringify([
      epoch,
      ready.value,
      current?.playId,
      current?.entryId,
      current?.queueRevision,
      current?.state,
      current?.error,
      current?.errorCode,
      current?.errorRetryable,
      reported.value ? reportedRevision.value : null,
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
    if (present && issue.code !== 'stale') {
      reportedRevision.value++;
      reported.value = issue;
    }
    if (['session_expired', 'profile_changed'].includes(issue.code)) onSessionChanged();
    return issue;
  }
  function resetLyrics() {
    lyricRequest++;
    lyricProjectionPending = false;
    lyricProjectionRequest = undefined;
    lyrics.value = undefined;
    lyricLoading.value = false;
    lyricUpdating.value = false;
    lyricError.value = '';
  }
  function accept(next: PlayerView, refreshQueue = true) {
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
      resetLyrics();
      if (lyricsOpen.value && next.playId) void loadLyrics();
    }
    const queueChanged = old?.queueRevision !== next.queueRevision;
    const moreAvailabilityChanged = !!old?.queueMoreAvailable !== !!next.queueMoreAvailable;
    if (queueChanged || moreAvailabilityChanged) {
      queueMoreError.value = '';
      if (refreshQueue && queueOpen.value) void loadQueue();
    }
  }
  async function connect(account: AccountView) {
    if (epoch !== account.epoch) lyricsOpen.value = false;
    const version = ++connection;
    disconnect?.();
    disconnect = undefined;
    clearSkipNotices();
    epoch = account.epoch;
    request++;
    failedOperation = undefined;
    queueRequest++;
    resetLyrics();
    ready.value = false;
    busy.value = false;
    pendingSourceCommand.value = undefined;
    loadingStepCommand.value = undefined;
    queueBusy.value = false;
    queueLoading.value = false;
    queueEditing.value = false;
    queueError.value = '';
    queueEditError.value = '';
    queueMoreError.value = '';
    queueMoreChanging.value = false;
    queuePreparation++;
    view.value = undefined;
    queue.value = undefined;
    reported.value = undefined;
    let registered = false,
      ended = false,
      nativeOpenFailed = false;
    function finish() {
      // The channel's initial state and the invoke reply can arrive in
      // either order. Don't miss startup or re-run it on later snapshots.
      if (version !== connection || !registered || ended || !view.value || ready.value) return;
      ready.value = true;
      if (restorable.value && !nativeOpenFailed)
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
          if (event.nativeOpenError) {
            nativeOpenFailed = true;
            handle(event.nativeOpenError);
          }
          void finish();
        } else if (event.kind === 'ordinary_copyright_skip') {
          showSkipNotice(event);
        } else {
          ended = true;
          clearSkipNotices();
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
  async function run(
    operation: PlaybackOperation,
    onRetry: PlaybackOperation | null = operation,
    selectionScoped: boolean | ((initial?: PlayerView, current?: PlayerView) => boolean) = false,
  ) {
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
        const currentSelection =
          typeof selectionScoped === 'function'
            ? selectionScoped(origin, view.value)
            : !selectionScoped || sameSelection(origin, view.value);
        const issue = handle(
          onRetry === null ? { ...failure(e), retryable: false } : e,
          currentSelection,
        );
        failedOperation =
          currentSelection && issue.code !== 'stale'
            ? {
                repeat: issue.retryable ? (onRetry ?? undefined) : undefined,
                selection: selectionScoped ? view.value : undefined,
              }
            : undefined;
      }
    } finally {
      if (current === request) busy.value = false;
    }
  }
  function action(input: PlayerAction) {
    const invoke = (e: string) => core.playerAction(e, input);
    if (input.kind === 'refresh_for') return run(invoke, invoke, true);
    // A search menu selection is bound to one result and queue snapshot.
    // Reopening the menu is a new intent; the player must not retry the old one.
    if (input.kind === 'search_group_next_play' || input.kind === 'search_group_play')
      return run(invoke, null, true);
    if (input.kind !== 'quality_for') return run(invoke);
    const origin = view.value;
    const failedReload = (initial?: PlayerView, current?: PlayerView) =>
      !!initial?.entryId &&
      current?.epoch === initial.epoch &&
      current.queueRevision === initial.queueRevision &&
      current.entryId === initial.entryId &&
      !!current.playId &&
      current.playId !== initial.playId &&
      current.state === 'error' &&
      current.requestedQuality === input.level;
    return run(
      invoke,
      async (e) => {
        const intent = request;
        await syncPlayback();
        if (e !== epoch || intent !== request)
          throw { code: 'stale', message: '播放请求已更新', retryable: false };
        const current = view.value;
        // Authorization errors leave the original player intact. A newly
        // authorized decoder failure has a new play ID for the same entry.
        if (current?.playId && failedReload(origin, current))
          return core.playerAction(e, {
            kind: 'refresh_for',
            play_id: current.playId,
            expected_queue_revision: current.queueRevision,
          });
        if (current?.playId !== input.play_id || !sameSelection(origin, current))
          throw { code: 'stale', message: '播放请求已更新', retryable: false };
        return invoke(e);
      },
      (initial, current) => sameSelection(initial, current) || failedReload(initial, current),
    );
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
  function queueSortedPlaylist(
    scope: import('../services/playlistDuration').PlaylistDurationScope,
  ) {
    return queueAction({
      kind: 'queue_sorted_playlist',
      scope,
      quality: view.value?.qualityPreferences.quality ?? 'standard',
    });
  }
  function queueOfflinePlaylist(id: string, revision: string, cached_at_ms: number) {
    return queueAction({ kind: 'queue_offline_playlist', id, revision, cached_at_ms });
  }
  function queueMixedPlaylist(
    id: string,
    revision: string,
    track: { id: string; local: boolean } | null,
  ) {
    return queueAction({
      kind: 'queue_mixed_playlist',
      id,
      revision,
      track,
      quality: view.value?.qualityPreferences.quality ?? 'standard',
    });
  }
  function queueZonePlaylistDetail(
    origin: { id: string; receipt: string; revision: string },
    song_id?: string,
  ) {
    const quality = view.value?.qualityPreferences.quality ?? 'standard';
    return queueAction(
      song_id === undefined
        ? { kind: 'queue_vertical_zone_playlist_all', ...origin, quality }
        : { kind: 'queue_vertical_zone_playlist_next', ...origin, quality, song_id },
    );
  }
  function queueZonePlaylistBatch(
    selection: Omit<Extract<PlaySelection, { kind: 'vertical_zone_playlist_batch' }>, 'kind'>,
  ) {
    return queueAction({ kind: 'queue_vertical_zone_playlist_batch', ...selection });
  }
  function queueVoice(id: string, program?: import('../services/podcast').PodcastProgramTarget) {
    return queueAction({ kind: 'queue_voice', id, ...(program ? { program } : {}) });
  }
  function queueRecentProgram(target: import('../services/podcast').PodcastRecentProgramTarget) {
    return queueAction({ kind: 'queue_recent_program', target });
  }
  function queuePurchasedProgram(target: import('../services/podcast').PodcastPurchasedTarget) {
    return queueAction({ kind: 'queue_purchased_program', target });
  }
  function queueLikedProgram(target: import('../services/podcast').PodcastLikedProgramTarget) {
    return queueAction({ kind: 'queue_liked_program', target });
  }
  function queueLikedPageProgram(target: import('../services/podcast').PodcastLikedPageTarget) {
    return queueAction({ kind: 'queue_liked_page_program', target });
  }
  function queueLocal(query: LocalQuery, revision: string, trackId?: string) {
    return queueAction({ kind: 'queue_local', query, revision, track_id: trackId });
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
  function queueAlbum(
    id: string,
    selection?: { ids: string[]; revision: string },
    detail?: import('../services/albumDurationSort').AlbumPlaybackScope,
  ) {
    const quality = view.value?.qualityPreferences.quality ?? 'standard';
    return queueAction(
      selection
        ? { kind: 'queue_album_batch', id, quality, ...selection }
        : detail
          ? { kind: 'queue_album_detail', album: detail, quality }
          : { kind: 'queue_album', id, quality },
    );
  }
  // A toolbar may first load pages or dismiss a modal. Capture intent now
  // so the delayed handoff cannot target a newer queue or selection.
  function captureQueueHandoff() {
    const captured = epoch,
      session = connection,
      intent = request,
      before = view.value;
    return () => {
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
    };
  }
  function prepareCloudQueue() {
    const ensureCurrent = captureQueueHandoff();
    return (ids: string[], revision: string, query: CloudQuery) => {
      ensureCurrent();
      return queueCloud(ids, revision, query);
    };
  }
  function prepareArtistSongsBatch() {
    const ensureCurrent = captureQueueHandoff();
    return (pages: CatalogSelectionPage[], ids: string[]) => {
      ensureCurrent();
      return queueAction({
        kind: 'queue_artist_songs_batch',
        pages,
        ids,
        quality: view.value?.qualityPreferences.quality ?? 'standard',
      });
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
            | 'queue_sorted_playlist'
            | 'queue_offline_playlist'
            | 'queue_mixed_playlist'
            | 'queue_voice'
            | 'queue_recent_program'
            | 'queue_purchased_program'
            | 'queue_liked_program'
            | 'queue_liked_page_program'
            | 'queue_daily_recommendations'
            | 'queue_style_recommendations'
            | 'queue_playlist_batch'
            | 'queue_vertical_zone_playlist_batch'
            | 'queue_vertical_zone_playlist_all'
            | 'queue_vertical_zone_playlist_next'
            | 'queue_cloud_batch'
            | 'queue_album_batch'
            | 'queue_album'
            | 'queue_album_detail'
            | 'queue_artist_songs'
            | 'queue_artist_songs_batch';
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
  // Absolute playback preferences have their own feedback. Saving one must
  // not supersede an in-flight transport or become the audio-error retry.
  async function configurePreference(
    input: Extract<PlayerAction, { kind: 'volume' | 'speed' | 'speed_for' | 'song_speed' }>,
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
  const configureVolume = (value: number) => configurePreference({ kind: 'volume', value });
  const configureSpeed = (value: number) =>
    configurePreference({
      kind: 'speed_for',
      scope: view.value?.speedScope ?? (view.value?.track?.voiceId ? 'voice' : 'song'),
      value,
    });
  const configureSongSpeed = (input: Extract<PlayerAction, { kind: 'song_speed' }>) =>
    configurePreference(input);
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
  async function playVinylSimilar(
    seed: VinylSimilarSeed,
    receipt: string,
    action: VinylSimilarAction,
  ) {
    const selected = view.value;
    if (
      !ready.value ||
      busy.value ||
      seed.epoch !== epoch ||
      selected?.playId !== seed.playId ||
      selected.track?.id !== seed.songId
    )
      return false;
    const quality = selected.qualityPreferences.quality;
    // A play action is tied to the visible native receipt. A general player
    // retry must never replay it after the source or recommendation view changes.
    const next = await run(
      (e) => core.playerVinylSimilarPlay(e, seed.playId, receipt, action, quality),
      null,
      true,
    );
    return !!next && next.state !== 'error';
  }
  async function runVoiceSongAction(
    scope: VoiceSongsScope,
    action: (epoch: string, quality: Quality) => Promise<PlayerView>,
  ) {
    const selected = view.value;
    if (
      !ready.value ||
      busy.value ||
      scope.epoch !== epoch ||
      selected?.playId !== scope.playId ||
      selected.voiceDetail?.id !== scope.voiceId
    )
      return false;
    const quality = selected.qualityPreferences.quality;
    // A later retry would no longer be a click on the same episode card.
    const next = await run((e) => action(e, quality), null, true);
    return !!next && next.state !== 'error';
  }
  function playVoiceSong(scope: VoiceSongsScope, receipt: string, action: VoiceSongAction) {
    return runVoiceSongAction(scope, (e, quality) =>
      core.playerVoiceSongPlay(e, scope.playId, receipt, action, quality),
    );
  }
  function playVoiceSongSelection(
    scope: VoiceSongsScope,
    receipt: string,
    action: VoiceSongSelectionAction,
  ) {
    return runVoiceSongAction(scope, (e, quality) =>
      core.playerVoiceSongsSelection(e, scope.playId, receipt, action, quality),
    );
  }
  async function controlPendingSource(current: PlayerView, input: PlayerAction) {
    if (pendingTransportBusy.value || loadingStepBusy.value) return;
    // The original selection still owns busy/retry until decode completes.
    const captured = epoch;
    const session = connection;
    const command = { source: current };
    pendingSourceCommand.value = command;
    try {
      const next = await core.playerAction(captured, input);
      if (session === connection && captured === epoch) accept(next);
    } catch (e) {
      if (session === connection && captured === epoch)
        handle({ ...failure(e), retryable: false }, sameSelection(current, view.value));
    } finally {
      if (pendingSourceCommand.value === command) pendingSourceCommand.value = undefined;
    }
  }
  async function seekDisplayed(
    target: Pick<PlayerView, 'epoch' | 'playId' | 'queueRevision'>,
    seconds: number,
  ) {
    const current = view.value;
    const loading =
      current?.state === 'loading' &&
      !current.fm &&
      current.timeline?.end != null &&
      current.timeline.end > current.timeline.start;
    if (
      !ready.value ||
      (!loading && (busy.value || !loaded.value)) ||
      !target.playId ||
      target.epoch !== current?.epoch ||
      target.playId !== current.playId ||
      target.queueRevision !== current.queueRevision
    )
      return;
    const input = {
      kind: 'seek_for',
      play_id: target.playId,
      expected_queue_revision: target.queueRevision,
      seconds,
    } as const;
    return loading ? controlPendingSource(current, input) : action(input);
  }
  function togglePrimaryTransport(displayedKey: string | undefined) {
    const current = view.value;
    if (!current || !ready.value || displayedKey !== primaryTransportKey.value) return;
    const pending = pendingTransport.value;
    if (pending)
      return controlPendingSource(current, {
        kind: 'pause_for',
        play_id: pending.playId,
        expected_queue_revision: pending.revision,
        paused: !pending.paused,
      });
    if (busy.value || current.state === 'preparing' || current.state === 'loading') return;
    if (loaded.value && !current.fm) return toggleSource();
    // An inactive selection needs a new load; FM play uses its coordinator.
    return action(playing.value ? { kind: 'pause', paused: true } : { kind: 'play' });
  }
  async function stepDisplayed(direction: 'next' | 'previous', displayedKey?: string) {
    const current = view.value;
    if (!ready.value || !current?.queueLen) return;
    if (endedStepKey.value) {
      if (
        busy.value ||
        displayedKey !== endedStepKey.value ||
        !current.endedMinibarReceipt ||
        !current.entryId
      )
        return;
      return action({
        kind: 'step_ended_mini_bar',
        receipt: current.endedMinibarReceipt,
        entry_id: current.entryId,
        expected_queue_revision: current.queueRevision,
        direction,
      });
    }
    if (!displayedKey) {
      if (busy.value || current.state === 'preparing' || current.state === 'loading') return;
      if (
        current.playId &&
        (loaded.value || current.state === 'ended') &&
        !current.fm &&
        !current.intelligence
      ) {
        return action({
          kind: 'step_mini_bar',
          play_id: current.playId,
          expected_queue_revision: current.queueRevision,
          direction,
        });
      }
      return action({ kind: direction });
    }
    if (
      !current.playId ||
      current.queueLen < 2 ||
      current.fm ||
      current.intelligence ||
      displayedKey !== stepIdentity(current) ||
      pendingTransportBusy.value ||
      loadingStepCommand.value?.key === displayedKey
    )
      return;
    const playId = current.playId;
    const command = { key: displayedKey };
    loadingStepCommand.value = command;
    try {
      return await run(
        (e) =>
          core.playerAction(e, {
            kind: 'step_loading',
            play_id: playId,
            expected_queue_revision: current.queueRevision,
            direction,
          }),
        // If the next entry fails, the native retry belongs to that entry,
        // never to a repeated skip of the queue.
        null,
        true,
      );
    } finally {
      if (loadingStepCommand.value === command) loadingStepCommand.value = undefined;
    }
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
  async function runSelectionForPage(select: (epoch: string) => Promise<PlayerView>) {
    if (!ready.value || busy.value || queueBusy.value) return;
    const captured = epoch,
      session = connection,
      current = ++request;
    busy.value = true;
    reported.value = undefined;
    failedOperation = undefined;
    try {
      const next = await select(captured);
      if (captured === epoch && session === connection) accept(next);
      if (captured === epoch && session === connection && current === request) return next;
    } catch (issue) {
      if (captured === epoch && session === connection && current === request) throw issue;
    } finally {
      if (current === request) busy.value = false;
    }
  }
  const selectForPage = (input: PlaySelection) =>
    runSelectionForPage((e) => core.playSelection(e, input));
  // Acquire native ownership before reading the card's list. The renderer check
  // avoids redundant reads; native also sees newer intent before events arrive.
  const preparedSelection =
    (prepare: (epoch: string) => Promise<PlaySelection>) => async (e: string) => {
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
    };
  const prepareSelection = (prepare: (epoch: string) => Promise<PlaySelection>) =>
    runSelection(preparedSelection(prepare));
  const prepareSelectionForPage = (prepare: (epoch: string) => Promise<PlaySelection>) =>
    runSelectionForPage(preparedSelection(prepare));
  const downloads = (ids: string[], index: number) =>
    runSelection((e) => core.playDownloads(e, ids, index));
  function retry(displayedKey = retryKey.value) {
    if (!canRetry.value || busy.value || displayedKey !== retryKey.value) return;
    const failed = failedOperation;
    if (failed?.repeat) return run(failed.repeat, failed.repeat, !!failed.selection);
    const current = view.value;
    if (current?.state === 'error' && current.playId)
      return action({
        kind: 'refresh_for',
        play_id: current.playId,
        expected_queue_revision: current.queueRevision,
      });
    return action({ kind: 'refresh' });
  }
  function playMissingDownloadOnline(displayedKey: string) {
    const current = view.value;
    if (
      !canPlayMissingDownloadOnline.value ||
      busy.value ||
      displayedKey !== retryKey.value ||
      !current?.playId
    )
      return;
    const input: PlayerAction = {
      kind: 'play_missing_download_online',
      play_id: current.playId,
      expected_queue_revision: current.queueRevision,
    };
    // Leave this choice in its own action; general Retry cannot opt in to networking.
    return run((owner) => core.playerAction(owner, input), null, true);
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
  async function sendQueueMore(displayed: QueueView, enabled?: boolean): Promise<boolean> {
    const recommendations = queueMoreRecommendations(displayed);
    const toggle = enabled !== undefined;
    if (
      !recommendations ||
      !queueOpen.value ||
      !ready.value ||
      displayed !== queue.value ||
      displayed.epoch !== epoch ||
      displayed.revision !== view.value?.queueRevision ||
      view.value.fm ||
      view.value.queueMoreAvailable === false ||
      (toggle
        ? !recommendations.canToggle ||
          recommendations.enabled === enabled ||
          queueMoreChanging.value ||
          busy.value ||
          queueBusy.value ||
          queueEditing.value ||
          queueLoading.value ||
          !!queueError.value
        : !recommendations.enabled || !recommendations.prefetchOnOpen)
    )
      return false;

    const captured = epoch,
      session = connection;
    const active = () => session === connection && captured === epoch && ready.value;
    if (toggle) {
      queueMoreChanging.value = true;
      queueMoreError.value = '';
    }
    try {
      const next = await core.playerAction(
        captured,
        enabled === undefined
          ? { kind: 'queue_more_opened', expected_revision: displayed.revision }
          : { kind: 'queue_more_toggle', enabled, expected_revision: displayed.revision },
      );
      if (!active() || next.epoch !== captured) return false;
      accept(next, !toggle);
      // A preference-only acknowledgement may leave the queue revision
      // unchanged. Read the native projection before displaying the new value.
      if (toggle && queueOpen.value) await loadQueue();
      return active();
    } catch (issue) {
      if (active()) {
        const problem = failure(issue);
        if (toggle && queueOpen.value) {
          if (['queue_changed', 'snapshot_changed', 'stale'].includes(problem.code))
            await loadQueue();
          if (active() && queueOpen.value) queueMoreError.value = problem.message;
        }
        if (active() && ['session_expired', 'profile_changed'].includes(problem.code))
          onSessionChanged();
      }
      return false;
    } finally {
      if (toggle && session === connection && captured === epoch) queueMoreChanging.value = false;
    }
  }
  function queueMoreOpened(displayed: QueueView) {
    return sendQueueMore(displayed);
  }
  function toggleQueueMore(displayed: QueueView, enabled: boolean) {
    return sendQueueMore(displayed, enabled);
  }
  function clearQueueMoreError() {
    queueMoreError.value = '';
  }
  // Queue edits belong to the list the user saw, with their own pending/error
  // feedback. They must never become the footer's audio-error retry operation.
  async function editQueue(input: QueueEdit, displayed = queue.value): Promise<boolean> {
    return sendQueueEdit(
      displayed,
      (snapshot) => ({
        kind: 'queue_for',
        expected_queue_revision: snapshot.revision,
        ...(input.kind === 'clear' ? { expected_room_id: snapshot.clearContext.roomId } : {}),
        edit: input,
      }),
      input.kind === 'clear',
    );
  }
  async function dropIntoQueue(
    source: QueueDropSource,
    displayed: QueueView,
    playId: string | null,
    before: string | null,
  ) {
    if (!source.current() || (view.value?.playId ?? null) !== playId) return false;
    return sendQueueEdit(
      displayed,
      (snapshot) => {
        const target = {
          expected_queue_revision: snapshot.revision,
          expected_play_id: playId,
          before_entry_id: before,
        };
        if (source.kind === 'local') {
          return {
            kind: 'drop_local_into_queue',
            query: source.query,
            revision: source.revision,
            track_id: source.trackId,
            ...target,
          };
        }
        if (source.kind === 'search')
          return {
            kind: 'drop_search_songs_into_queue',
            rows: source.rows.map(({ receipt, trackId }) => ({ receipt, track_id: trackId })),
            quality: source.quality,
            ...target,
          };
        if (source.kind === 'mixed_playlist')
          return {
            kind: 'drop_mixed_playlist_songs_into_queue',
            id: source.id,
            revision: source.revision,
            rows: source.rows.map(({ trackId, trackIndex, local }) => ({
              track_id: trackId,
              track_index: trackIndex,
              local,
            })),
            quality: source.quality,
            ...target,
          };
        const rows = source.rows.map(({ trackId, trackIndex }) => ({
          track_id: trackId,
          track_index: trackIndex,
        }));
        if (source.kind === 'album')
          return {
            kind: 'drop_album_songs_into_queue',
            id: source.id,
            revision: source.revision,
            quality: source.quality,
            rows,
            ...target,
          };
        const playlist = { id: source.id, revision: source.revision, quality: source.quality };
        return rows.length === 1
          ? { kind: 'drop_playlist_song_into_queue', ...rows[0], ...playlist, ...target }
          : { kind: 'drop_playlist_songs_into_queue', rows, ...playlist, ...target };
      },
      false,
      (problem) => {
        if (source.kind !== 'playlist' && source.kind !== 'mixed_playlist') return false;
        if (problem.code === 'playlist_drop_source_changed') {
          source.sourceChanged();
          return true;
        }
        if (problem.code === 'snapshot_changed') {
          queueEditError.value = '播放列表已更新，请核对当前列表后重试';
          return true;
        }
        return false;
      },
    );
  }
  async function sendQueueEdit(
    displayed: QueueView | undefined | null,
    action: (snapshot: QueueView) => PlayerAction,
    rereadOnFailure = false,
    handleFailure?: (problem: CoreFailure) => boolean,
  ): Promise<boolean> {
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
      const next = await core.playerAction(captured, action(displayed));
      if (session !== connection || captured !== epoch || !ready.value) return false;
      accept(next);
      return true;
    } catch (issue) {
      if (session === connection && captured === epoch) {
        const problem = failure(issue);
        if (!handleFailure?.(problem)) queueEditError.value = problem.message;
        if (['session_expired', 'profile_changed'].includes(problem.code)) onSessionChanged();
        // Re-read room policy after a rejected confirmation. This never
        // retries the clear or adopts a new room as the old confirmation.
        if (rereadOnFailure) await loadQueue();
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
    if (!playId || lyricLoading.value || lyricUpdating.value) return;
    const current = ++lyricRequest,
      captured = epoch;
    lyricProjectionPending = false;
    lyricLoading.value = true;
    lyricError.value = '';
    try {
      const value = await core.playerLyrics(captured, playId);
      if (current === lyricRequest && value.epoch === epoch && value.playId === view.value?.playId)
        lyrics.value = value;
    } catch (e) {
      if (current === lyricRequest) lyricError.value = failure(e).message;
    } finally {
      if (current === lyricRequest) {
        lyricLoading.value = false;
        void refreshLyricProjection();
      }
    }
  }
  function toggleLyrics() {
    lyricsOpen.value = !lyricsOpen.value;
    if (lyricsOpen.value && !lyrics.value) void loadLyrics();
    else if (lyricsOpen.value) void refreshLyricProjection();
  }
  function observeLyricChange(target: { epoch: string; playId: string }) {
    if (target.epoch !== epoch || target.playId !== view.value?.playId) return;
    lyricProjectionPending = true;
    void refreshLyricProjection();
  }
  async function refreshLyricProjection() {
    const playId = view.value?.playId;
    if (
      !playId ||
      !lyricsOpen.value ||
      !lyricProjectionPending ||
      lyricProjectionRequest ||
      lyricLoading.value ||
      lyricUpdating.value
    )
      return;
    const reading = Symbol();
    const version = lyricRequest;
    const captured = epoch;
    lyricProjectionPending = false;
    lyricProjectionRequest = reading;
    try {
      const value = await core.playerLyrics(captured, playId, true);
      if (lyricProjectionRequest !== reading) return;
      // A local gesture may have committed after this projection was made.
      // Read the current cached document once the newer action has settled.
      if (version !== lyricRequest) lyricProjectionPending = true;
      else if (value.epoch === epoch && value.playId === view.value?.playId) {
        if (!lyrics.value) lyricError.value = '';
        lyrics.value = value;
      }
    } catch {
      // A notification is advisory. Keep the last readable document and
      // leave any explicit retry with the user; this never fetches remotely.
    } finally {
      if (lyricProjectionRequest === reading) {
        lyricProjectionRequest = undefined;
        void refreshLyricProjection();
      }
    }
  }
  async function updateLyrics(operation: (epoch: string, playId: string) => Promise<LyricsView>) {
    const playId = view.value?.playId;
    if (
      !playId ||
      lyricLoading.value ||
      lyricUpdating.value ||
      lyrics.value?.playId !== playId ||
      lyrics.value.epoch !== epoch
    )
      return false;
    const current = ++lyricRequest,
      captured = epoch;
    lyricUpdating.value = true;
    lyricError.value = '';
    try {
      const value = await operation(captured, playId);
      if (
        current === lyricRequest &&
        value.epoch === epoch &&
        value.playId === view.value?.playId
      ) {
        lyrics.value = value;
        return true;
      }
    } catch (e) {
      if (current === lyricRequest) lyricError.value = failure(e).message;
    } finally {
      if (current === lyricRequest) {
        lyricUpdating.value = false;
        void refreshLyricProjection();
      }
    }
    return false;
  }
  const preferences = (value: LyricPreferences) =>
    updateLyrics((e, id) => core.lyricPreferences(e, id, value));
  const offset = (gain: number) => updateLyrics((e, id) => core.lyricOffset(e, id, gain));
  function dispose() {
    equalizerDraft.reset();
    clearSkipNotices();
    connection++;
    request++;
    resetLyrics();
    queueRequest++;
    queuePreparation++;
    queueBusy.value = false;
    queueLoading.value = false;
    queueEditing.value = false;
    queueMoreChanging.value = false;
    queueMoreError.value = '';
    disconnect?.();
    ready.value = false;
  }
  return {
    queuePlaylist,
    queueSortedPlaylist,
    queueOfflinePlaylist,
    queueMixedPlaylist,
    queueZonePlaylistBatch,
    queueZonePlaylistDetail,
    queueVoice,
    queueRecentProgram,
    queuePurchasedProgram,
    queueLikedProgram,
    queueLikedPageProgram,
    queueLocal,
    dropIntoQueue,
    queueAlbum,
    queueDaily,
    queueStyleRecommendations,
    queueArtistSongs,
    prepareArtistSongsBatch,
    queueDownloads,
    queueCloud,
    prepareCloudQueue,
    prepareSelection,
    prepareSelectionForPage,
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
    ordinarySkipNotices,
    errorCode,
    canRetry,
    queueError,
    queueEditError,
    queueMoreChanging,
    queueMoreError,
    lyricError,
    queueOpen,
    lyricsOpen,
    lyricLoading,
    lyricUpdating,
    playing,
    loaded,
    connect,
    action,
    qualityTrial,
    configureLoudness,
    configureEqualizer,
    equalizerDraft,
    configureVolume,
    configureSpeed,
    configureSongSpeed,
    configureMode,
    intelligence,
    syncPlayback,
    select,
    playVinylSimilar,
    playVoiceSong,
    playVoiceSongSelection,
    toggleSource,
    primaryTransportKey,
    primaryTransportPauses,
    pendingTransport,
    pendingTransportBusy,
    loadingStepKey,
    endedStepKey,
    loadingStepBusy,
    stepDisplayed,
    togglePrimaryTransport,
    seekDisplayed,
    togglePlaylist,
    selectForPage,
    downloads,
    retry,
    retryKey,
    canPlayMissingDownloadOnline,
    playMissingDownloadOnline,
    loadQueue,
    queueMoreOpened,
    toggleQueueMore,
    clearQueueMoreError,
    editQueue,
    toggleQueue,
    loadLyrics,
    observeLyricChange,
    toggleLyrics,
    preferences,
    offset,
    dispose,
  };
}
export type PlayerModel = ReturnType<typeof usePlayer>;
