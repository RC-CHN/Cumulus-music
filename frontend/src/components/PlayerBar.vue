<script setup lang="ts">
import { computed, ref, nextTick, useId, watch } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import type { IntelligenceModel } from '../composables/useIntelligence';
import { currentCatalogDownload, time } from '../services/player';
import type { PlayerAction } from '../services/player';
import type { AccountView, TrackView } from '../services/contracts';
import Icon from './Icon.vue';
import EqualizerPanel from './EqualizerPanel.vue';
import ModalTransition from './ModalTransition.vue';
import PlaybackSpeed from './PlaybackSpeed.vue';
import PlaybackQuality from './PlaybackQuality.vue';
import PlaybackVolume from './PlaybackVolume.vue';
import PlaybackMode from './PlaybackMode.vue';
import { artistLabel } from '../services/artistLabel';
import { effectLauncher, equalizerLauncher } from '../services/equalizer';
import { bufferedSeekSpan, insidePlayableTrial, seekRail } from '../services/seekRail';
const equalizerOpen = ref(false);
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
  }>(),
  { togetherActive: false },
);
const emit = defineEmits<{
  like: [track: TrackView];
  retry: [];
  comments: [track: TrackView];
  download: [epoch: string, playId: string, trackId: string];
  member: [];
  login: [];
}>();
const { view, ready, busy, error, canRetry, playing, loaded } = props.player;
const equalizerDescription = useId();
const equalizerEntry = computed(() => equalizerLauncher(view.value?.equalizer));
const effectEntry = computed(() =>
  effectLauncher(view.value?.equalizer, view.value?.boundParametric),
);
function loginFromEffects() {
  equalizerOpen.value = false;
  emit('login');
}
const currentDownload = computed(() =>
  props.downloadsAllowed ? currentCatalogDownload(view.value) : undefined,
);
function downloadCurrent() {
  const selection = currentDownload.value;
  if (selection) emit('download', selection.epoch, selection.playId, selection.track.id);
}
const transportControls = ref<HTMLElement>();
async function retryPlayback(event: MouseEvent) {
  const origin = event.currentTarget as HTMLButtonElement,
    owner = view.value?.epoch,
    host = transportControls.value,
    feedback = origin.closest('.playback-message'),
    key = origin.dataset.retryKey;
  if (!key || key !== props.player.retryKey.value) return;
  if (document.activeElement === origin) host?.focus({ preventScroll: true });
  if (ready.value) await props.player.retry(key);
  else emit('retry');
  await nextTick();
  if (owner === view.value?.epoch && document.activeElement === host) {
    const target =
      (feedback?.isConnected
        ? feedback.querySelector<HTMLButtonElement>('button:not(:disabled)')
        : null) ?? host?.querySelector<HTMLButtonElement>('.play-toggle:not(:disabled)');
    target?.focus({ preventScroll: true });
  }
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
  const key = (event.currentTarget as HTMLButtonElement).dataset.primaryTransportKey;
  return transport(event, () => props.player.togglePrimaryTransport(key));
}
const draft = ref<number>();
const outsideTrial = ref(false);
const rail = computed(() => seekRail(view.value?.timeline));
const seekValue = computed(() => draft.value ?? view.value?.position ?? rail.value.min);
const buffered = computed(() =>
  draft.value === undefined
    ? bufferedSeekSpan(view.value?.timeline, view.value?.position, view.value?.bufferedThrough)
    : null,
);
const layeredSeek = computed(
  () => view.value?.timeline?.end != null && rail.value.max > rail.value.min,
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
async function seek(event: Event) {
  const input = event.currentTarget as HTMLInputElement;
  const current = view.value;
  const key = scrubKey.value;
  if (!current || input.dataset.scrubKey !== key) return;
  const seconds = Number(input.value);
  if (!insidePlayableTrial(current.timeline, seconds)) {
    outsideTrial.value = current.timeline?.mode === 'trial';
    draft.value = undefined;
    input.value = String(current.position ?? rail.value.min);
    return;
  }
  outsideTrial.value = false;
  draft.value = seconds;
  await props.player.seekDisplayed(current, seconds);
  if (scrubKey.value === key && draft.value === seconds) draft.value = undefined;
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
        aria-label="下载当前歌曲"
        title="下载当前歌曲"
        @click="downloadCurrent"
      >
        <Icon name="download" />
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
          :disabled="!ready || !view?.queueLen || pending"
          @click="transport($event, { kind: 'previous' })"
        >
          <Icon name="previous" />
        </button>
        <button
          class="play-toggle"
          :aria-label="playing ? '暂停' : '播放'"
          :disabled="!ready || (!view?.queueLen && !view?.track) || pending"
          :data-primary-transport-key="player.primaryTransportKey.value"
          @click="primaryTransport"
        >
          <Icon :name="playing ? 'pause' : 'play'" />
        </button>
        <button
          class="icon-button"
          aria-label="下一首"
          :disabled="!ready || !view?.queueLen || pending"
          @click="transport($event, { kind: 'next' })"
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
        <div class="seek-rail" :class="{ 'seek-rail-layered': layeredSeek }">
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
                : undefined
            "
            type="range"
            :min="rail.min"
            :max="rail.max"
            step="0.1"
            :value="seekValue"
            :disabled="!loaded || !view?.timeline?.end || pending"
            @input="scrubInput"
            @pointercancel="cancelScrub"
            @change="seek"
          />
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
        class="icon-button equalizer-toggle"
        aria-label="均衡器"
        aria-haspopup="dialog"
        :aria-describedby="equalizerDescription"
        :data-equalizer-state="equalizerEntry.state"
        :data-parametric-state="view?.boundParametric"
        :title="effectEntry.label"
        :disabled="!ready"
        @click="equalizerOpen = true"
      >
        {{ view?.boundParametric === 'configured' ? 'PEQ' : 'EQ'
        }}<span v-if="effectEntry.state === 'problem'" class="equalizer-marker" aria-hidden="true"
          >!</span
        >
        <span
          v-else-if="effectEntry.state === 'processing' || view?.boundParametric === 'configured'"
          class="equalizer-marker"
          aria-hidden="true"
          >•</span
        >
      </button>
      <span :id="equalizerDescription" class="sr-only">{{ effectEntry.label }}</span>
      <button
        class="queue-toggle icon-button"
        aria-label="播放队列"
        :aria-pressed="player.queueOpen.value"
        :disabled="!ready"
        @click="player.toggleQueue"
      >
        <Icon name="list" /><small>{{ view?.queueLen ?? 0 }}</small>
      </button>
      <button
        v-if="view?.track && !view.track.local"
        class="icon-button"
        aria-label="当前歌曲评论"
        @click="emit('comments', view.track)"
      >
        <Icon name="comment" />
      </button>
    </div>
    <div class="player-feedback-stack">
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
  <ModalTransition>
    <EqualizerPanel
      v-if="equalizerOpen"
      :account="account"
      :player="player"
      @close="equalizerOpen = false"
      @login="loginFromEffects"
    />
  </ModalTransition>
</template>
