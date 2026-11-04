<script setup lang="ts">
import { computed, ref, shallowRef, nextTick, useId, watch, onUnmounted } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import type { IntelligenceModel } from '../composables/useIntelligence';
import { currentCatalogTrack, currentPlaybackDownload, time } from '../services/player';
import type {
  CurrentCatalogTrack,
  CurrentPlaybackDownload,
  PlayerAction,
} from '../services/player';
import type { AccountView, TrackView } from '../services/contracts';
import type { CatalogResource } from '../services/catalog';
import Icon from './Icon.vue';
import EqualizerPanel from './EqualizerPanel.vue';
import EffectDirectory from './EffectDirectory.vue';
import ModalTransition from './ModalTransition.vue';
import PlaybackSpeed from './PlaybackSpeed.vue';
import PlaybackQuality from './PlaybackQuality.vue';
import PlaybackVolume from './PlaybackVolume.vue';
import PlaybackMode from './PlaybackMode.vue';
import { artistLabel } from '../services/artistLabel';
import { effectLauncher, equalizerLauncher } from '../services/equalizer';
import { headphoneTuningLabel } from '../services/effectDirectory';
import { bufferedSeekSpan, insidePlayableTrial, seekRail } from '../services/seekRail';
import { usePlaybackChorus } from '../composables/usePlaybackChorus';
import { usePlaybackCommentCount } from '../composables/usePlaybackCommentCount';
import { usePlaybackDownloadState } from '../composables/usePlaybackDownloadState';
import { useQueueMoreTip } from '../composables/useQueueMoreTip';
import { formatPlaybackCommentCount } from '../services/playbackCommentCount';
import PlayerTrackActions from './PlayerTrackActions.vue';
const equalizerOpen = ref(false);
const effectDirectoryOpen = ref(false);
const headphoneManagementOpen = ref(false);
const effectLauncherButton = ref<HTMLButtonElement>();
const effectManagementOnOpen = ref(false);
const props = withDefaults(
  defineProps<{
    player: PlayerModel;
    account: AccountView;
    liked: Set<string>;
    likeBusy: Set<string>;
    togetherActive?: boolean;
    intelligence?: IntelligenceModel;
    likedPlaylistId?: string | null;
    downloadsAllowed?: boolean;
    downloadsChanging?: boolean;
  }>(),
  { togetherActive: false },
);
const emit = defineEmits<{
  like: [track: TrackView];
  retry: [];
  comments: [track: TrackView];
  download: [target: CurrentPlaybackDownload];
  member: [];
  login: [];
  audioOutput: [epoch: string];
  catalog: [resource: CatalogResource];
}>();
const {
  view,
  ready,
  busy,
  error,
  canRetry,
  loaded,
  pendingTransport,
  pendingTransportBusy,
  primaryTransportPauses,
  loadingStepKey,
  loadingStepBusy,
} = props.player;
const chorusMarker = usePlaybackChorus(view, loaded);
const showQueueMoreTip = useQueueMoreTip(
  view,
  () => props.account.epoch,
  () => props.account.status === 'authenticated' && !props.togetherActive,
);
const commentCount = usePlaybackCommentCount(view, loaded, () => props.account.epoch);
const commentBadge = computed(() => formatPlaybackCommentCount(commentCount.value));
const commentDescription = useId();
const equalizerDescription = useId();
const equalizerEntry = computed(() => equalizerLauncher(view.value?.equalizer));
const headphone = computed(() => {
  const current = view.value;
  return props.account.status === 'authenticated' && current?.epoch === props.account.epoch
    ? current.deviceParametric
    : undefined;
});
const effectEntry = computed(() => {
  const status = headphone.value?.status;
  const current = view.value;
  const processing = current?.parametricProcessing;
  if (!status) return effectLauncher(current?.equalizer, current?.boundParametric, processing);
  const eqProblem = equalizerEntry.value.state === 'problem';
  const label = headphoneTuningLabel(status, processing);
  return {
    state:
      eqProblem || status === 'detach_required' || status === 'processor_failed'
        ? 'problem'
        : status === 'deferred'
          ? 'pending'
          : status === 'paused_locally'
            ? equalizerEntry.value.state
            : processing
              ? 'processing'
              : 'configured',
    label: eqProblem ? label + '；' + equalizerEntry.value.label : label,
  };
});
function openEqualizer() {
  headphoneManagementOpen.value = false;
  effectManagementOnOpen.value = !!view.value?.boundParametric;
  equalizerOpen.value = true;
}
function openEffectLauncher() {
  if (headphone.value) headphoneManagementOpen.value = true;
  else openEqualizer();
}
function loginFromEffects() {
  equalizerOpen.value = false;
  effectDirectoryOpen.value = false;
  headphoneManagementOpen.value = false;
  emit('login');
}
const currentCatalog = computed<CurrentCatalogTrack | undefined>((previous) => {
  const current = currentCatalogTrack(view.value);
  if (
    current &&
    previous?.epoch === current.epoch &&
    previous.playId === current.playId &&
    previous.track.id === current.track.id
  )
    return previous;
  return current;
});
const currentDownload = computed<CurrentPlaybackDownload | undefined>((previous) => {
  if (!props.downloadsAllowed || view.value?.epoch !== props.account.epoch) return undefined;
  const target = currentPlaybackDownload(view.value);
  return target &&
    previous?.epoch === target.epoch &&
    previous.playId === target.playId &&
    previous.kind === target.kind &&
    previous.id === target.id &&
    previous.name === target.name
    ? previous
    : target;
});
const { state: downloadState, retry: retryDownloadState } = usePlaybackDownloadState(
  currentDownload,
  () => !!props.downloadsChanging,
);
const downloadResourceLabel = computed(() =>
  currentDownload.value?.kind === 'voice' ? '声音' : '歌曲',
);
const downloadNotice = ref('');
let downloadNoticeTimer: ReturnType<typeof setTimeout> | undefined;
function clearDownloadNotice() {
  clearTimeout(downloadNoticeTimer);
  downloadNotice.value = '';
}
watch([currentDownload, downloadState], clearDownloadNotice);
onUnmounted(clearDownloadNotice);
const downloadLabel = computed(
  () =>
    ({
      reading: `正在读取当前${downloadResourceLabel.value}下载状态`,
      unknown: `重新读取当前${downloadResourceLabel.value}下载状态`,
      none: `下载当前${downloadResourceLabel.value}`,
      waiting: `等待下载当前${downloadResourceLabel.value}`,
      working: `正在下载当前${downloadResourceLabel.value}`,
      complete: `当前${downloadResourceLabel.value}已下载`,
      paused: `当前${downloadResourceLabel.value}下载已暂停`,
      failed: `当前${downloadResourceLabel.value}下载失败`,
    })[downloadState.value ?? 'reading'],
);
function downloadCurrent() {
  const selection = currentDownload.value;
  if (!selection || props.downloadsChanging) return;
  const state = downloadState.value;
  if (!state || state === 'working') return;
  if (state === 'unknown') retryDownloadState();
  else if (state === 'none') emit('download', selection);
  else {
    clearDownloadNotice();
    downloadNotice.value =
      state === 'waiting'
        ? '等待下载中'
        : state === 'complete'
          ? `该${downloadResourceLabel.value}已下载`
          : state === 'paused'
            ? '下载已暂停，请到下载管理继续'
            : '下载失败，请到下载管理重试';
    downloadNoticeTimer = setTimeout(clearDownloadNotice, 3000);
  }
}
const transportControls = ref<HTMLElement>();
async function playbackFeedbackAction(event: MouseEvent, action: (key: string) => unknown) {
  const origin = event.currentTarget as HTMLButtonElement,
    owner = view.value?.epoch,
    host = transportControls.value,
    feedback = origin.closest('.playback-message'),
    key = origin.dataset.retryKey;
  if (!key || key !== props.player.retryKey.value) return;
  if (document.activeElement === origin) host?.focus({ preventScroll: true });
  await action(key);
  await nextTick();
  if (owner === view.value?.epoch && document.activeElement === host) {
    const target =
      (feedback?.isConnected
        ? feedback.querySelector<HTMLButtonElement>('button:not(:disabled)')
        : null) ?? host?.querySelector<HTMLButtonElement>('.play-toggle:not(:disabled)');
    target?.focus({ preventScroll: true });
  }
}
function retryPlayback(event: MouseEvent) {
  return playbackFeedbackAction(event, (key) =>
    ready.value ? props.player.retry(key) : emit('retry'),
  );
}
function playMissingDownloadOnline(event: MouseEvent) {
  if (!props.togetherActive)
    return playbackFeedbackAction(event, props.player.playMissingDownloadOnline);
}
function openAudioOutput(event: MouseEvent) {
  const key = (event.currentTarget as HTMLButtonElement).dataset.retryKey;
  if (key === props.player.retryKey.value && props.player.errorCode.value === 'audio_output_failed')
    emit('audioOutput', props.account.epoch);
}
async function transport(event: MouseEvent, action: PlayerAction | (() => unknown)) {
  const origin = event.currentTarget as HTMLButtonElement,
    owner = view.value?.epoch,
    host = transportControls.value;
  if (document.activeElement === origin) host?.focus({ preventScroll: true });
  if (typeof action === 'function') await action();
  else await props.player.action(action);
  await nextTick();
  if (
    owner === view.value?.epoch &&
    document.activeElement === host &&
    origin.isConnected &&
    !origin.disabled
  )
    origin.focus({ preventScroll: true });
}
function primaryTransport(event: MouseEvent) {
  if (pendingTransport.value && props.togetherActive) return;
  const key = (event.currentTarget as HTMLButtonElement).dataset.primaryTransportKey;
  return transport(event, () => props.player.togglePrimaryTransport(key));
}
function stepTransport(event: MouseEvent, direction: 'next' | 'previous') {
  const key = (event.currentTarget as HTMLButtonElement).dataset.loadingStepKey;
  if (key && props.togetherActive) return;
  return transport(event, () => props.player.stepDisplayed(direction, key));
}
const draft = ref<number>();
const seekHovered = ref(false);
const seekInFlight = shallowRef<{ key: string }>();
const outsideTrial = ref(false);
const rail = computed(() => seekRail(view.value?.timeline));
const seekStep = computed(() => {
  const length = rail.value.max - rail.value.min;
  return Number.isFinite(length) && length > 0 ? length / 1000 : 0.1;
});
const seekValue = computed(() => draft.value ?? view.value?.position ?? rail.value.min);
const seekTotalText = computed(() => {
  const total = view.value?.timeline?.total;
  return total == null ? '—' : time(total);
});
const buffered = computed(() =>
  draft.value === undefined
    ? bufferedSeekSpan(view.value?.timeline, view.value?.position, view.value?.bufferedThrough)
    : null,
);
const layeredSeek = computed(
  () => view.value?.timeline?.end != null && rail.value.max > rail.value.min,
);
const canSeek = computed(
  () =>
    ready.value &&
    layeredSeek.value &&
    !seekInFlight.value &&
    ((loaded.value && !busy.value) ||
      (view.value?.state === 'loading' &&
        !view.value.fm &&
        !props.togetherActive &&
        !pendingTransportBusy.value &&
        !loadingStepBusy.value)),
);
const seekPlayed = computed(() => {
  const { span, min, max } = rail.value;
  if (max <= min) return 0;
  const position = (100 * (seekValue.value - min)) / (max - min);
  return Math.min(span?.right ?? 100, Math.max(span?.left ?? 0, position));
});
// Replacing a playback session also replaces its slider. A late event from
// the old input still carries its own identity, not the new song's view.
const scrubKey = computed(() => {
  const current = view.value;
  const trial = current?.timeline?.mode === 'trial' ? current.timeline : undefined;
  return JSON.stringify([
    current?.epoch,
    current?.playId,
    current?.queueRevision,
    trial?.start,
    trial?.end,
    trial?.total,
  ]);
});
const slowPlayback = ref(false);
watch(
  () => {
    const current = view.value;
    if (
      !current?.playId ||
      (!['preparing', 'loading', 'buffering'].includes(current.state) &&
        !current.nativeSeeking &&
        !(loaded.value && seekInFlight.value?.key === scrubKey.value))
    )
      return;
    return JSON.stringify([current.epoch, current.playId]);
  },
  (owner, _old, onCleanup) => {
    slowPlayback.value = false;
    if (!owner) return;
    // Native progress may update many times during one continuous wait.
    // Only completion or a replacement selection restarts this deadline.
    const timer = setTimeout(() => (slowPlayback.value = true), 3000);
    onCleanup(() => clearTimeout(timer));
  },
  { immediate: true, flush: 'sync' },
);
watch(
  scrubKey,
  () => {
    draft.value = undefined;
    outsideTrial.value = false;
  },
  { flush: 'sync' },
);
const intelligenceNotice = ref<HTMLElement>();
let cancelFocus: { epoch: string; control: Element | null } | undefined;
function focusIntelligence(epoch: string, control: Element | null) {
  void nextTick(() => {
    if (
      view.value?.epoch !== epoch ||
      (document.activeElement !== control && document.activeElement !== document.body)
    )
      return;
    const next =
      intelligenceNotice.value?.querySelector<HTMLElement>('button:not(:disabled)') ??
      document.querySelector<HTMLElement>('.intelligence-start:not(:disabled)') ??
      document.querySelector<HTMLElement>('.mode-control > button:not(:disabled)');
    next?.focus({ preventScroll: true });
  });
}
function cancelIntelligence() {
  if (!props.intelligence || !view.value) return;
  cancelFocus = { epoch: view.value.epoch, control: document.activeElement };
  void props.intelligence.cancel();
}
watch(
  () => props.intelligence?.busy.value,
  (value, old) => {
    if (!value && old && cancelFocus) {
      const target = cancelFocus;
      cancelFocus = undefined;
      focusIntelligence(target.epoch, target.control);
    }
  },
  { flush: 'post' },
);
function dismissIntelligence() {
  const epoch = view.value?.epoch,
    control = document.activeElement;
  props.intelligence?.dismiss();
  if (epoch) focusIntelligence(epoch, control);
}
const pending = computed(
  () => busy.value || ['preparing', 'loading'].includes(view.value?.state ?? ''),
);
const canStep = computed(
  () =>
    ready.value &&
    !!view.value?.queueLen &&
    (!pending.value ||
      (!!loadingStepKey.value &&
        !props.togetherActive &&
        !pendingTransportBusy.value &&
        !loadingStepBusy.value)),
);
const message = computed(
  () =>
    error.value ||
    view.value?.error ||
    (outsideTrial.value
      ? '只能在试听片段内调整播放进度'
      : pending.value
        ? '正在准备音乐…'
        : view.value?.state === 'buffering'
          ? '正在缓冲…'
          : view.value?.positionReset
            ? '音源可播范围已变化，进度已重置'
            : (view.value?.historyWarning ?? '')),
);
function scrubInput(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  if (input.dataset.scrubKey !== scrubKey.value) return;
  outsideTrial.value = false;
  draft.value = Number(input.value);
}
function cancelScrub(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  if (input.dataset.scrubKey === scrubKey.value) draft.value = undefined;
}
function showSeekTime(event: PointerEvent) {
  if (event.pointerType !== 'touch') seekHovered.value = true;
}
async function seek(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  const current = view.value;
  const key = scrubKey.value;
  if (!canSeek.value || !current || input.dataset.scrubKey !== key) return;
  const seconds = Number(input.value);
  if (!insidePlayableTrial(current.timeline, seconds)) {
    outsideTrial.value = current.timeline?.mode === 'trial';
    draft.value = undefined;
    input.value = String(current.position ?? rail.value.min);
    return;
  }
  outsideTrial.value = false;
  draft.value = seconds;
  const operation = { key };
  seekInFlight.value = operation;
  try {
    await props.player.seekDisplayed(current, seconds);
  } finally {
    if (seekInFlight.value === operation) {
      seekInFlight.value = undefined;
      if (scrubKey.value === key && draft.value === seconds) draft.value = undefined;
    }
  }
}
</script>
<template>
  <footer
    class="playerbar"
    :data-state="view?.state ?? 'idle'"
    :data-position="view?.position ?? 0"
    :data-volume="view?.volume"
    :data-unmute-volume="view?.unmuteVolume"
    :data-mode="view?.mode"
    :data-intelligence-phase="view?.intelligence"
    :data-intelligence-operation="view?.intelligenceOperation"
    :data-fm="view?.fm"
    :data-quality="view?.actualQuality"
    :data-requested-quality="view?.requestedQuality"
    :data-preferred-quality="view?.qualityPreferences.quality"
    :data-quality-preferences-damaged="view?.qualityPreferences.damaged"
    :data-play-id="view?.playId"
    :data-track-id="view?.track?.id"
    :data-speed="view?.speed"
    aria-label="播放器"
  >
    <div class="now-playing">
      <button
        class="player-cover"
        :disabled="!view?.track"
        aria-label="打开播放详情"
        @click="player.toggleLyrics"
      >
        <img
          v-if="view?.track?.coverUrl"
          :src="view.track.coverUrl"
          alt=""
          referrerpolicy="no-referrer"
        /><Icon v-else name="music" />
      </button>
      <div class="now-copy">
        <button
          class="ellipsis"
          :title="view?.track?.name ?? '云间音乐'"
          :disabled="!view?.track"
          @click="player.toggleLyrics"
        >
          {{ view?.track?.name ?? '云间音乐' }}</button
        ><small class="ellipsis" :title="artistLabel(view?.track, '发现每一首喜欢的歌')">{{
          artistLabel(view?.track, '发现每一首喜欢的歌')
        }}</small>
      </div>
      <button
        v-if="view?.track && !view.track.local"
        class="icon-button like-button"
        :class="{ liked: liked.has(view.track.id) }"
        aria-label="喜欢当前歌曲"
        :aria-pressed="liked.has(view.track.id)"
        :disabled="likeBusy.has(view.track.id)"
        @click="emit('like', view.track)"
      >
        <Icon name="heart" />
      </button>
      <button
        v-if="currentDownload"
        type="button"
        class="icon-button player-current-download"
        :data-download-state="downloadState ?? 'reading'"
        :aria-label="downloadLabel"
        :title="downloadLabel"
        :disabled="!downloadState || downloadState === 'working' || downloadsChanging"
        @click="downloadCurrent"
      >
        <span
          v-if="downloadState === 'working'"
          class="player-download-spinner"
          aria-hidden="true"
        />
        <Icon
          v-else
          :name="
            downloadState === 'complete'
              ? 'check'
              : downloadState === 'unknown'
                ? 'refresh'
                : 'download'
          "
        />
      </button>
    </div>
    <div class="player-center">
      <div
        ref="transportControls"
        class="transport-controls"
        tabindex="-1"
        role="group"
        aria-label="播放控制"
      >
        <PlaybackMode
          :player="player"
          :together-active="togetherActive"
          :intelligence="intelligence"
          :liked-playlist-id="likedPlaylistId"
        />
        <button
          class="icon-button"
          aria-label="上一首"
          :disabled="!canStep"
          :data-loading-step-key="loadingStepKey"
          @click="stepTransport($event, 'previous')"
        >
          <Icon name="previous" />
        </button>
        <button
          class="play-toggle"
          :aria-label="primaryTransportPauses ? '暂停' : '播放'"
          :disabled="
            !ready ||
            (!view?.queueLen && !view?.track) ||
            (pendingTransport ? togetherActive || pendingTransportBusy || loadingStepBusy : pending)
          "
          :data-primary-transport-key="player.primaryTransportKey.value"
          @click="primaryTransport"
        >
          <Icon :name="primaryTransportPauses ? 'pause' : 'play'" />
        </button>
        <button
          class="icon-button"
          aria-label="下一首"
          :disabled="!canStep"
          :data-loading-step-key="loadingStepKey"
          @click="stepTransport($event, 'next')"
        >
          <Icon name="next" />
        </button>
        <button
          class="icon-button"
          aria-label="停止播放"
          :disabled="!ready"
          @click="transport($event, { kind: 'stop' })"
        >
          <Icon name="stop" />
        </button>
      </div>
      <div class="seek-control">
        <time>{{ time(draft ?? view?.position) }}</time>
        <div
          class="seek-rail"
          :class="{ 'seek-rail-layered': layeredSeek, 'seek-rail-stalled': slowPlayback }"
          @pointerenter="showSeekTime"
          @pointerleave="seekHovered = false"
        >
          <div v-if="layeredSeek" class="trial-seek-track" aria-hidden="true">
            <span
              v-if="rail.span"
              class="trial-seek-window"
              :style="{ left: rail.span.left + '%', right: 100 - rail.span.right + '%' }"
            />
            <span
              v-if="buffered"
              class="seek-buffered"
              :style="{ left: buffered.left + '%', right: 100 - buffered.right + '%' }"
            />
            <span
              class="trial-seek-played"
              :style="{
                left: (rail.span?.left ?? 0) + '%',
                right: 100 - seekPlayed + '%',
              }"
            />
            <span
              v-if="chorusMarker != null"
              class="seek-chorus"
              :class="{ reached: seekPlayed >= chorusMarker }"
              :style="{ left: chorusMarker + '%' }"
            />
            <template v-if="rail.span">
              <span class="trial-seek-start" :style="{ left: rail.span.left + '%' }" />
              <span class="trial-seek-end" :style="{ left: rail.span.right + '%' }" />
            </template>
          </div>
          <input
            :key="scrubKey"
            :data-scrub-key="scrubKey"
            aria-label="播放进度"
            :aria-valuetext="
              rail.trial
                ? `${time(seekValue)}，可试听 ${time(rail.trial.start)} 至 ${time(rail.trial.end)}`
                : `${time(seekValue)} / ${seekTotalText}`
            "
            type="range"
            :min="rail.min"
            :max="rail.max"
            :step="seekStep"
            :value="seekValue"
            :disabled="!canSeek"
            @input="scrubInput"
            @pointercancel="cancelScrub"
            @change="seek"
          />
          <span v-if="slowPlayback" class="seek-load-rail" aria-hidden="true">
            <span class="seek-load-indicator" :style="{ left: seekPlayed + '%' }" />
          </span>
          <span
            v-if="loaded && layeredSeek"
            class="seek-time-preview"
            :class="{ visible: seekHovered || draft !== undefined }"
            :style="{ '--seek-preview-position': seekPlayed + '%' }"
            aria-hidden="true"
          >
            {{ time(seekValue) }} / {{ seekTotalText }}
          </span>
        </div>
        <time>{{ time(view?.timeline?.total) }}</time>
      </div>
    </div>
    <div class="player-tools">
      <PlaybackQuality :player="player" @member="emit('member')" />
      <PlaybackSpeed :player="player" />
      <PlaybackVolume :player="player" />
      <button
        class="icon-button"
        aria-label="歌词"
        :aria-pressed="player.lyricsOpen.value"
        :disabled="!view?.playId"
        @click="player.toggleLyrics"
      >
        词
      </button>
      <button
        ref="effectLauncherButton"
        class="icon-button equalizer-toggle"
        :aria-label="
          headphone ? '管理耳机音效' : view?.boundParametric ? '管理独立参数均衡' : '均衡器'
        "
        aria-haspopup="dialog"
        :aria-describedby="equalizerDescription"
        :data-equalizer-state="equalizerEntry.state"
        :data-effect-state="effectEntry.state"
        :data-parametric-state="view?.boundParametric"
        :data-headphone-state="headphone?.status"
        :title="effectEntry.label"
        :disabled="!ready"
        @click="openEffectLauncher"
      >
        {{ headphone ? '耳机' : view?.boundParametric === 'configured' ? 'PEQ' : 'EQ'
        }}<span v-if="effectEntry.state === 'problem'" class="equalizer-marker" aria-hidden="true"
          >!</span
        >
        <span
          v-else-if="
            effectEntry.state === 'processing' ||
            headphone?.status === 'configured' ||
            view?.boundParametric === 'configured'
          "
          class="equalizer-marker"
          aria-hidden="true"
          >•</span
        >
      </button>
      <span :id="equalizerDescription" class="sr-only">{{ effectEntry.label }}</span>
      <div class="queue-more-tip-anchor">
        <button
          class="queue-toggle icon-button"
          aria-label="播放队列"
          :aria-pressed="player.queueOpen.value"
          :disabled="!ready"
          @click="player.toggleQueue"
        >
          <Icon name="list" /><small>{{ view?.queueLen ?? 0 }}</small>
        </button>
        <Transition name="queue-more-tip">
          <span v-if="showQueueMoreTip" class="queue-more-tip" data-queue-more-tip role="status">
            列表播完已自动推荐，↓点击去设置
          </span>
        </Transition>
      </div>
      <button
        v-if="view?.track && !view.track.local"
        class="icon-button player-comment"
        aria-label="当前歌曲评论"
        :aria-describedby="commentBadge ? commentDescription : undefined"
        @click="emit('comments', view.track)"
      >
        <Icon name="comment" />
        <span v-if="commentBadge" class="player-comment-count" aria-hidden="true">{{
          commentBadge
        }}</span>
      </button>
      <span v-if="commentBadge" :id="commentDescription" class="sr-only"
        >{{ commentCount }} 条评论</span
      >
      <PlayerTrackActions
        :selection="currentCatalog"
        :account-epoch="account.epoch"
        :effects-available="ready"
        @catalog="(resource) => emit('catalog', resource)"
        @effects="effectDirectoryOpen = true"
      />
    </div>
    <div class="player-feedback-stack">
      <Transition name="playback-feedback">
        <div v-if="downloadNotice" class="playback-message player-download-notice" role="status">
          <span>{{ downloadNotice }}</span>
        </div>
      </Transition>
      <div v-if="message" class="playback-message" role="status">
        <span>{{ message }}</span
        ><button
          v-if="canRetry"
          class="text-button"
          :disabled="busy"
          :data-retry-key="player.retryKey.value"
          @click="retryPlayback"
        >
          重试</button
        ><button
          v-if="player.canPlayMissingDownloadOnline.value && !togetherActive"
          type="button"
          class="text-button"
          :disabled="busy"
          :data-retry-key="player.retryKey.value"
          @click="playMissingDownloadOnline"
        >
          在线播放</button
        ><button
          v-if="player.errorCode.value === 'audio_output_failed'"
          type="button"
          class="text-button"
          :disabled="busy"
          :data-retry-key="player.retryKey.value"
          @click="openAudioOutput"
        >
          音频输出设置</button
        ><span v-if="view?.timeline?.mode === 'trial' && !outsideTrial">试听片段</span>
      </div>
      <Transition
        name="playback-feedback"
        @before-leave="(element) => ((element as HTMLElement).inert = true)"
      >
        <div
          v-if="
            intelligence &&
            (intelligence.busy.value || intelligence.issue.value || intelligence.resumable.value)
          "
          class="playback-message intelligence-message"
          ref="intelligenceNotice"
          role="status"
        >
          <span>{{
            intelligence.busy.value
              ? intelligence.cancelling.value
                ? '正在取消推荐…'
                : intelligence.progress.value === 'preparing'
                  ? '正在准备心动模式，可继续操作播放器'
                  : '正在获取心动推荐，可继续操作播放器'
              : (intelligence.issue.value?.message ?? '心动推荐尚未完成')
          }}</span>
          <button
            v-if="intelligence.busy.value"
            class="text-button"
            :disabled="intelligence.cancelling.value"
            @click="cancelIntelligence"
          >
            取消推荐
          </button>
          <button
            v-else-if="intelligence.resumable.value"
            class="text-button"
            @click="intelligence.resume"
          >
            继续获取推荐
          </button>
          <button
            v-if="!intelligence.busy.value && intelligence.issue.value"
            class="text-button"
            @click="dismissIntelligence"
            aria-label="关闭心动模式提示"
          >
            关闭
          </button>
        </div>
      </Transition>
    </div>
  </footer>
  <ModalTransition :return-focus="() => effectLauncherButton ?? null">
    <EqualizerPanel
      v-if="equalizerOpen"
      :account="account"
      :player="player"
      :initial-effect-management="effectManagementOnOpen"
      @close="equalizerOpen = false"
      @login="loginFromEffects"
    />
  </ModalTransition>
  <ModalTransition :return-focus="() => effectLauncherButton ?? null">
    <EffectDirectory
      v-if="headphoneManagementOpen"
      :account="account"
      :binding-status="view?.boundParametric"
      :parametric-processing="view?.parametricProcessing"
      :device-parametric="headphone"
      headphone-management
      @close="headphoneManagementOpen = false"
      @equalizer="openEqualizer"
      @login="loginFromEffects"
    />
  </ModalTransition>
  <ModalTransition>
    <EffectDirectory
      v-if="effectDirectoryOpen"
      :account="account"
      :binding-status="view?.boundParametric"
      :parametric-processing="view?.parametricProcessing"
      :device-parametric="view?.deviceParametric"
      @close="effectDirectoryOpen = false"
      @login="loginFromEffects"
    />
  </ModalTransition>
</template>
