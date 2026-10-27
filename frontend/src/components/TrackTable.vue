<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from 'vue';
import type { TrackView } from '../services/contracts';
import Icon from './Icon.vue';
const props = defineProps<{
  tracks: TrackView[];
  liked: Set<string>;
  busy: Set<string>;
  playable?: boolean;
  currentId?: string;
}>();
const emit = defineEmits<{
  like: [track: TrackView];
  play: [index: number];
  more: [track: TrackView];
}>();
const scroller = ref<HTMLDivElement>();
const scroll = ref(0);
const height = ref(500);
const rowHeight = 56;
const start = computed(() => Math.max(0, Math.floor(scroll.value / rowHeight) - 6));
const end = computed(() =>
  Math.min(props.tracks.length, Math.ceil((scroll.value + height.value) / rowHeight) + 6),
);
const rows = computed(() => props.tracks.slice(start.value, end.value));
let observer: ResizeObserver;
onMounted(() => {
  observer = new ResizeObserver((entries) => {
    height.value = entries[0]?.contentRect.height ?? 500;
  });
  if (scroller.value) observer.observe(scroller.value);
});
onUnmounted(() => observer?.disconnect());
watch(
  () => props.tracks,
  (_now, previous) => {
    if (props.tracks[0]?.id !== previous[0]?.id || props.tracks.length < previous.length) {
      scroll.value = 0;
      scroller.value?.scrollTo(0, 0);
    }
  },
);
function duration(ms: number) {
  const seconds = Math.floor(ms / 1000);
  return `${Math.floor(seconds / 60)
    .toString()
    .padStart(2, '0')}:${(seconds % 60).toString().padStart(2, '0')}`;
}
</script>
<template>
  <div class="track-table" role="table" :aria-rowcount="tracks.length + 1">
    <div class="track-columns track-header" role="row">
      <span>#</span><span role="columnheader">歌曲 / 歌手</span><span role="columnheader">专辑</span
      ><span role="columnheader">喜欢</span><span role="columnheader">时长</span>
    </div>
    <div
      ref="scroller"
      class="track-scroll"
      @scroll="scroll = ($event.target as HTMLElement).scrollTop"
    >
      <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
      <div
        v-for="(track, index) in rows"
        :key="`${start + index}:${track.id}`"
        class="track-columns track-row"
        :class="{ unavailable: track.missing, current: track.id === currentId }"
        @dblclick="!track.missing && playable && emit('play', start + index)"
        role="row"
        :aria-rowindex="start + index + 2"
      >
        <span class="track-index" role="cell"
          ><button
            class="track-play icon-button"
            :aria-label="`播放 ${track.name}`"
            :disabled="!playable || track.missing"
            @click.stop="emit('play', start + index)"
            @dblclick.stop
          >
            <span>{{ (start + index + 1).toString().padStart(2, '0') }}</span
            ><Icon :name="track.id === currentId ? 'volume' : 'play'" /></button
        ></span>
        <div class="track-title" role="cell">
          <img
            v-if="track.coverUrl"
            :src="track.coverUrl"
            alt=""
            loading="lazy"
            referrerpolicy="no-referrer"
          /><span v-else class="track-art"><Icon name="music" /></span>
          <div>
            <div class="ellipsis">
              {{ track.name }} <span v-if="track.fee === 1" class="vip-tag">VIP</span>
            </div>
            <small class="ellipsis">{{
              track.artists.join(' / ') || (track.local ? '本地文件' : '未知歌手')
            }}</small>
          </div>
        </div>
        <span class="track-album ellipsis" role="cell" :title="track.album">{{ track.album }}</span>
        <span role="cell"
          ><button
            class="icon-button like-button"
            :class="{ liked: liked.has(track.id) }"
            :aria-label="`${liked.has(track.id) ? '取消喜欢' : '喜欢'} ${track.name}`"
            :aria-pressed="liked.has(track.id)"
            :disabled="track.missing || busy.has(track.id)"
            @click.stop="emit('like', track)"
            @dblclick.stop
          >
            <Icon name="heart" /></button
        ></span>
        <span class="track-time-actions" role="cell"
          ><span class="track-duration">{{ track.missing ? '—' : duration(track.durationMs) }}</span
          ><button
            class="icon-button track-more"
            :aria-label="`更多操作 ${track.name}`"
            @click.stop="emit('more', track)"
            @dblclick.stop
          >
            ···
          </button></span
        >
      </div>
      <div :style="{ height: `${(tracks.length - end) * rowHeight}px` }" aria-hidden="true" />
    </div>
  </div>
</template>
