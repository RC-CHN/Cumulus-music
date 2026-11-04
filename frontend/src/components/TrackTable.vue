<script setup lang="ts">
import type { TrackCurrentToggle, TrackMenuRequest, TrackPlayGesture } from '../services/trackMenu';
import { computed, nextTick, watch } from 'vue';
import { useVirtualRows } from '../composables/useVirtualRows';
import type { TrackView } from '../services/contracts';
import TrackRow from './TrackRow.vue';
const props = defineProps<{
  tracks: TrackView[];
  matchKeyword?: string;
  originalRecordings?: ReadonlySet<string>;
  liked: Set<string>;
  busy: Set<string>;
  playable?: boolean;
  currentId?: string;
  currentToggle?: TrackCurrentToggle;
  toggleBusy?: boolean;
  navigable?: boolean;
  locateIndex?: number;
  scrollScope?: string;
}>();
const emit = defineEmits<{
  like: [track: TrackView];
  play: [index: number, gesture?: TrackPlayGesture];
  toggleCurrent: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
  more: [request: TrackMenuRequest];
  artist: [id: string];
  album: [id: string];
  scrollEdge: [scope: string, scrolled: boolean, overflow: number];
}>();
const rowHeight = 56;
const {
  container: scroller,
  start,
  end,
  onScroll,
  reset,
  restore: restoreScroll,
  center,
} = useVirtualRows(() => props.tracks.length, rowHeight);
const rows = computed(() => props.tracks.slice(start.value, end.value));
let wasScrolled = false;
function scroll() {
  onScroll();
  const node = scroller.value;
  const scrolled = (node?.scrollTop ?? 0) > 0;
  if (node && props.scrollScope && scrolled !== wasScrolled)
    emit('scrollEdge', props.scrollScope, scrolled, node.scrollHeight - node.clientHeight);
  wasScrolled = scrolled;
}
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
defineExpose({ locate, restoreScroll });
</script>
<template>
  <div class="track-table" role="table" :aria-rowcount="tracks.length + 1">
    <div class="track-columns track-header" role="row">
      <span>#</span><span role="columnheader">歌曲 / 歌手</span><span role="columnheader">专辑</span
      ><span role="columnheader">喜欢</span><span role="columnheader">时长</span>
    </div>
    <div ref="scroller" class="track-scroll" @scroll.passive="scroll">
      <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
      <TrackRow
        v-for="(track, index) in rows"
        :key="`${start + index}:${track.id}`"
        v-memo="[
          track,
          start + index,
          currentId === track.id,
          currentToggle?.trackId === track.id ? currentToggle : undefined,
          currentToggle?.trackId === track.id && toggleBusy,
          liked.has(track.id),
          busy.has(track.id),
          playable,
          navigable,
          matchKeyword,
          originalRecordings?.has(track.id),
          locateIndex !== undefined,
        ]"
        :track="track"
        :tabindex="locateIndex !== undefined ? -1 : undefined"
        :match-keyword="matchKeyword"
        :original-recording="originalRecordings?.has(track.id)"
        :index="start + index"
        :current-id="currentId"
        :current-toggle="
          !track.missing && !track.local && currentToggle?.trackId === track.id
            ? currentToggle
            : undefined
        "
        :toggle-busy="toggleBusy"
        :liked="liked.has(track.id)"
        :busy="busy.has(track.id)"
        :playable="playable"
        :navigable="navigable"
        @play="(index, gesture) => emit('play', index, gesture)"
        @toggle-current="(target, anchor) => emit('toggleCurrent', target, anchor)"
        @like="emit('like', $event)"
        @more="emit('more', $event)"
        @artist="emit('artist', $event)"
        @album="emit('album', $event)"
      />
      <div :style="{ height: `${(tracks.length - end) * rowHeight}px` }" aria-hidden="true" />
    </div>
  </div>
</template>
