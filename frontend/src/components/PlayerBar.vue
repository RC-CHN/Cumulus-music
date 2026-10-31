<script setup lang="ts">
import { computed, ref } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { time } from '../services/player';
import type { TrackView } from '../services/contracts';
import Icon from './Icon.vue';
import EqualizerPanel from './EqualizerPanel.vue';
import ModalTransition from './ModalTransition.vue';
import PlaybackSpeed from './PlaybackSpeed.vue';
import PlaybackQuality from './PlaybackQuality.vue';
import PlaybackVolume from './PlaybackVolume.vue';
import PlaybackMode from './PlaybackMode.vue';
const equalizerOpen = ref(false);
const props = withDefaults(
  defineProps<{
    player: PlayerModel;
    liked: Set<string>;
    likeBusy: Set<string>;
    togetherActive?: boolean;
  }>(),
  { togetherActive: false },
);
const emit = defineEmits<{
  like: [track: TrackView];
  retry: [];
  comments: [track: TrackView];
  member: [];
}>();
const { view, ready, busy, error, playing, loaded } = props.player;
const draft = ref<number>();
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
          : (view.value?.historyWarning ?? '')),
);
async function seek(event: Event) {
  const seconds = Number((event.target as HTMLInputElement).value);
  await props.player.action({ kind: 'seek', seconds });
  draft.value = undefined;
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
        ><small
          class="ellipsis"
          :title="view?.track?.artists.join(' / ') || '发现每一首喜欢的歌'"
          >{{ view?.track?.artists.join(' / ') || '发现每一首喜欢的歌' }}</small
        >
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
        <PlaybackMode :player="player" :together-active="togetherActive" />
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
