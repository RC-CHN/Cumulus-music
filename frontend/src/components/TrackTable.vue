<script setup lang="ts">
import type { TrackMenuRequest, TrackPlayGesture } from '../services/trackMenu';
import { computed, nextTick, watch } from 'vue';
import { useVirtualRows } from '../composables/useVirtualRows';
import type { TrackView } from '../services/contracts';
import TrackRow from './TrackRow.vue';
const props = defineProps<{
  tracks: TrackView[];
  matchKeyword?: string;
  liked: Set<string>;
  busy: Set<string>;
  playable?: boolean;
  currentId?: string;
  navigable?: boolean;
  locateIndex?: number;
}>();
const emit = defineEmits<{
  like: [track: TrackView];
  play: [index: number, gesture?: TrackPlayGesture];
  more: [request: TrackMenuRequest];
  artist: [id: string];
  album: [id: string];
}>();
const rowHeight = 56;
const {
  container: scroller,
  start,
  end,
  onScroll,
  reset,
  center,
} = useVirtualRows(() => props.tracks.length, rowHeight);
const rows = computed(() => props.tracks.slice(start.value, end.value));
watch(
  () => props.tracks,
  (_now, previous) => {
    if (props.tracks[0]?.id !== previous[0]?.id || props.tracks.length < previous.length) {
      reset();
    }
  },
);
async function locate() {
  const index = props.locateIndex;
  if (index === undefined || index < 0) return;
  const target = props.tracks[index];
  if (!target || !center(index)) return;
  await nextTick();
  if (index !== props.locateIndex || props.tracks[index] !== target) return;
  scroller.value
    ?.querySelector<HTMLElement>(`[aria-rowindex="${index + 2}"]`)
    ?.focus({ preventScroll: true });
}
defineExpose({ locate });
</script>
<template>
  <div class="track-table" role="table" :aria-rowcount="tracks.length + 1">
    <div class="track-columns track-header" role="row">
      <span>#</span><span role="columnheader">歌曲 / 歌手</span><span role="columnheader">专辑</span
      ><span role="columnheader">喜欢</span><span role="columnheader">时长</span>
    </div>
    <div ref="scroller" class="track-scroll" @scroll.passive="onScroll">
      <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
      <TrackRow
        v-for="(track, index) in rows"
        :key="`${start + index}:${track.id}`"
        v-memo="[
          track,
          start + index,
          currentId === track.id,
          liked.has(track.id),
          busy.has(track.id),
          playable,
          navigable,
          matchKeyword,
          locateIndex !== undefined,
        ]"
        :track="track"
        :tabindex="locateIndex !== undefined ? -1 : undefined"
        :match-keyword="matchKeyword"
        :index="start + index"
        :current-id="currentId"
        :liked="liked.has(track.id)"
        :busy="busy.has(track.id)"
        :playable="playable"
        :navigable="navigable"
        @play="(index, gesture) => emit('play', index, gesture)"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
        @artist="emit('artist', $event)"
        @album="emit('album', $event)"
      />
      <div :style="{ height: `${(tracks.length - end) * rowHeight}px` }" aria-hidden="true" />
    </div>
  </div>
</template>
