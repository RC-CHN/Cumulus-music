<script setup lang="ts">
import { computed, ref } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { qualities, time, type PlaybackMode, type Quality } from '../services/player';
import type { TrackView } from '../services/contracts';
import Icon from './Icon.vue';
import EqualizerPanel from './EqualizerPanel.vue';
import ModalTransition from './ModalTransition.vue';
const equalizerOpen = ref(false);
const props = defineProps<{ player: PlayerModel; liked: Set<string>; likeBusy: Set<string> }>();
const emit = defineEmits<{ like: [track: TrackView]; retry: []; comments: [track: TrackView] }>();
const { view, ready, busy, error, playing, loaded } = props.player;
const draft = ref<number>();
const previousVolume = ref(50);
const normalModes: { value: PlaybackMode; label: string; icon: string }[] = [
  { value: 'order_loop', label: '列表循环', icon: 'repeat' },
  { value: 'single_loop', label: '单曲循环', icon: 'repeatOne' },
  { value: 'random', label: '随机播放', icon: 'shuffle' },
  { value: 'sequential', label: '顺序播放', icon: 'list' },
];
const modes = computed(() =>
  view.value?.fm
    ? [{ value: 'sequential' as PlaybackMode, label: '无限漫游', icon: 'repeat' }, normalModes[1]!]
    : normalModes,
);
const mode = computed(
  () => modes.value.find((m) => m.value === view.value?.mode) ?? modes.value[0]!,
);
const pending = computed(
  () => busy.value || ['preparing', 'loading'].includes(view.value?.state ?? ''),
);
const message = computed(
  () =>
    error.value ||
    view.value?.error ||
    (pending.value
      ? '正在准备音乐…'
      : view.value?.state === 'buffering'
        ? '正在缓冲…'
        : view.value?.positionReset
          ? '音源可播范围已变化，进度已重置'
          : ''),
);
function cycle() {
  const index = modes.value.findIndex((m) => m.value === view.value?.mode);
  void props.player.action({
    kind: 'mode',
    mode: modes.value[(index + 1) % modes.value.length]!.value,
  });
}
async function seek(event: Event) {
  const seconds = Number((event.target as HTMLInputElement).value);
  await props.player.action({ kind: 'seek', seconds });
  draft.value = undefined;
}
function mute() {
  const v = view.value?.volume ?? 0;
  if (v > 0) previousVolume.value = v;
  void props.player.action({ kind: 'volume', value: v > 0 ? 0 : previousVolume.value });
}
</script>
<template>
  <footer
    class="playerbar"
    :data-state="view?.state ?? 'idle'"
    :data-position="view?.position ?? 0"
    :data-volume="view?.volume"
    :data-quality="view?.actualQuality"
    :data-requested-quality="view?.requestedQuality"
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
        <button class="ellipsis" :disabled="!view?.track" @click="player.toggleLyrics">
          {{ view?.track?.name ?? '云间音乐' }}</button
        ><small class="ellipsis">{{
          view?.track?.artists.join(' / ') || '发现每一首喜欢的歌'
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
    </div>
    <div class="player-center">
      <div class="transport-controls">
        <button
          class="icon-button"
          :aria-label="mode.label"
          :title="mode.label"
          :disabled="!ready"
          @click="cycle"
        >
          <Icon :name="mode.icon" />
        </button>
        <button
          class="icon-button"
          aria-label="上一首"
          :disabled="!ready || !view?.queueLen || pending"
          @click="player.action({ kind: 'previous' })"
        >
          <Icon name="previous" />
        </button>
        <button
          class="play-toggle"
          :aria-label="playing ? '暂停' : '播放'"
          :disabled="!ready || !view?.queueLen || pending"
          @click="player.action(playing ? { kind: 'pause', paused: true } : { kind: 'play' })"
        >
          <Icon :name="playing ? 'pause' : 'play'" />
        </button>
        <button
          class="icon-button"
          aria-label="下一首"
          :disabled="!ready || !view?.queueLen || pending"
          @click="player.action({ kind: 'next' })"
        >
          <Icon name="next" />
        </button>
        <button
          class="icon-button"
          aria-label="停止播放"
          :disabled="!ready"
          @click="player.action({ kind: 'stop' })"
        >
          <Icon name="stop" />
        </button>
      </div>
      <div class="seek-control">
        <time>{{ time(draft ?? view?.position) }}</time
        ><input
          aria-label="播放进度"
          type="range"
          :min="view?.timeline?.start ?? 0"
          :max="view?.timeline?.end ?? 1"
          step="0.1"
          :value="draft ?? view?.position ?? 0"
          :disabled="!loaded || !view?.timeline?.end || pending"
          @input="draft = Number(($event.target as HTMLInputElement).value)"
          @change="seek"
        /><time>{{ time(view?.timeline?.total) }}</time>
      </div>
    </div>
    <div class="player-tools">
      <label
        v-if="!view?.track?.local"
        class="quality-control"
        :title="`实际音质：${qualities.find((q) => q.value === view?.actualQuality)?.label ?? '未播放'}`"
        ><span class="sr-only">音质</span
        ><select
          aria-label="音质"
          :value="view?.requestedQuality ?? 'standard'"
          :disabled="!loaded || pending"
          @change="
            player.action({
              kind: 'quality',
              level: ($event.target as HTMLSelectElement).value as Quality,
            })
          "
        >
          <option v-for="q in qualities" :key="q.value" :value="q.value">
            {{ q.label }}
          </option></select
        ><small
          v-if="view?.actualQuality && view.actualQuality !== view.requestedQuality"
          class="actual-quality"
          >实际{{
            qualities.find((q) => q.value === view?.actualQuality)?.label ?? view.actualQuality
          }}</small
        ></label
      >
      <label class="speed-control"
        ><span class="sr-only">播放速度</span
        ><select
          aria-label="播放速度"
          :value="view?.speed ?? 1"
          :disabled="!ready"
          @change="
            player.action({
              kind: 'speed',
              value: Number(($event.target as HTMLSelectElement).value),
            })
          "
        >
          <option
            v-for="speed in [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4]"
            :value="speed"
            :key="speed"
          >
            {{ speed }}×
          </option>
        </select></label
      >
      <div class="volume-control">
        <button
          class="icon-button"
          :aria-label="view?.volume === 0 ? '取消静音' : '静音'"
          :disabled="!ready"
          @click="mute"
        >
          <Icon :name="view?.volume === 0 ? 'muted' : 'volume'" /></button
        ><input
          aria-label="音量"
          type="range"
          min="0"
          max="100"
          :value="view?.volume ?? 100"
          :disabled="!ready"
          @change="
            player.action({
              kind: 'volume',
              value: Number(($event.target as HTMLInputElement).value),
            })
          "
        />
      </div>
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
        :aria-pressed="view?.equalizer?.settings.enabled ?? false"
        :disabled="!ready"
        @click="equalizerOpen = true"
      >
        EQ
      </button>
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
    <div v-if="message" class="playback-message" role="status">
      <span>{{ message }}</span
      ><button
        v-if="error || view?.error"
        class="text-button"
        @click="ready ? player.retry() : emit('retry')"
      >
        重试</button
      ><span v-if="view?.timeline?.mode === 'trial'">试听片段</span>
    </div>
  </footer>
  <ModalTransition>
    <EqualizerPanel v-if="equalizerOpen" :player="player" @close="equalizerOpen = false" />
  </ModalTransition>
</template>
