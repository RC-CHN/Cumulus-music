<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, useId, watch } from 'vue';
import type { VoiceSongsView } from '../services/voiceSongs';
import {
  allVoiceSongs,
  singleVoiceSong,
  voiceSongContext,
  voiceSongDown,
  voiceSongNeighbor,
  voiceSongSelectionSnapshot,
  voiceSongUp,
  type VoiceSongSelection,
} from '../services/voiceSongSelection';
import { rowWindow } from '../services/virtualWindow';
import VoiceSongRow from './VoiceSongRow.vue';

const props = defineProps<{
  page: VoiceSongsView;
  scroller?: HTMLElement;
  liked: ReadonlySet<string>;
  busyLikes: ReadonlySet<string>;
  transportBusy: boolean;
}>();
const emit = defineEmits<{
  play: [index: number, gesture: 'cover' | 'double_click', indices?: readonly number[]];
  like: [index: number];
  more: [index: number, event: MouseEvent | KeyboardEvent, indices?: readonly number[]];
  artist: [index: number, id: string];
}>();
const title = useId();
const grid = ref<HTMLElement>();
const selection = shallowRef<VoiceSongSelection>({ indices: new Set() });
const focused = ref(0);
const rowHeight = 60;
const total = computed(() => props.page.tracks.length);
const window = shallowRef(rowWindow(total.value, rowHeight, 400, 0));
watch(
  [() => props.page.playId, () => props.page.voiceId, () => props.page.receipt],
  () => {
    selection.value = { indices: new Set() };
    focused.value = 0;
  },
  { flush: 'sync' },
);

// The card shares the voice shelf's scrolling surface with the other cards.
// Keep spacer geometry on that surface instead of introducing another scrollbar.
watch(
  [grid, () => props.scroller, total],
  ([content, scroller], _previous, clean) => {
    if (!content || !scroller) return;
    let frame = 0;
    function update() {
      frame = 0;
      const box = scroller!.getBoundingClientRect();
      const top = content!.getBoundingClientRect().top;
      const height = scroller!.clientHeight;
      const relative = box.top - top;
      window.value = rowWindow(
        total.value,
        rowHeight,
        Math.min(height, Math.max(rowHeight, relative + height)),
        Math.max(0, relative),
        window.value,
      );
    }
    function schedule() {
      if (!frame) frame = requestAnimationFrame(update);
    }
    const resize = new ResizeObserver(schedule);
    resize.observe(scroller);
    if (content.parentElement?.parentElement) resize.observe(content.parentElement.parentElement);
    scroller.addEventListener('scroll', schedule, { passive: true });
    update();
    clean(() => {
      cancelAnimationFrame(frame);
      resize.disconnect();
      scroller.removeEventListener('scroll', schedule);
    });
  },
  { flush: 'post', immediate: true },
);

const visibleRows = computed(() => {
  const start = window.value.start;
  return props.page.tracks.slice(start, window.value.end).map((track, offset) => ({
    index: start + offset,
    track,
    rights: props.page.availability[start + offset]!,
  }));
});
function select(
  index: number,
  gesture: 'down' | 'up' | 'activate',
  event: MouseEvent | KeyboardEvent,
) {
  if (gesture === 'activate') {
    focused.value = index;
    selection.value = singleVoiceSong(index);
    return;
  }
  if (!(event instanceof MouseEvent)) return;
  const modifiers = { button: event.button, toggle: event.ctrlKey, range: event.shiftKey };
  if (gesture === 'down') {
    if (event.button === 0) focused.value = index;
    selection.value = voiceSongDown(selection.value, index, total.value, modifiers);
  } else {
    selection.value = voiceSongUp(selection.value, index, modifiers);
  }
}
function play(index: number, gesture: 'cover' | 'double_click') {
  const indices =
    gesture === 'double_click' ? voiceSongSelectionSnapshot(selection.value, index) : undefined;
  emit('play', index, gesture, indices);
}
function context(index: number, event: MouseEvent | KeyboardEvent) {
  focused.value = index;
  selection.value = voiceSongContext(
    selection.value,
    index,
    event instanceof MouseEvent && (event.ctrlKey || event.shiftKey),
  );
  emit('more', index, event, voiceSongSelectionSnapshot(selection.value, index));
}
async function moveFocus(event: KeyboardEvent) {
  if (!(event.target instanceof HTMLElement) || !event.target.hasAttribute('data-voice-song-row'))
    return;
  if (event.ctrlKey && !event.altKey && event.key.toLowerCase() === 'a') {
    event.preventDefault();
    event.stopPropagation();
    selection.value = allVoiceSongs(selection.value, total.value);
    return;
  }
  if (event.shiftKey || event.ctrlKey || event.metaKey || event.altKey) return;
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  event.stopPropagation();
  const index =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? total.value - 1
        : voiceSongNeighbor(
            selection.value,
            focused.value,
            total.value,
            event.key === 'ArrowDown' ? 1 : -1,
          );
  focused.value = index;
  selection.value =
    event.key === 'Home' || event.key === 'End'
      ? singleVoiceSong(index)
      : { indices: new Set([index]), anchor: selection.value.anchor };
  const page = props.page;
  if (!grid.value?.querySelector(`[data-row-index="${index}"]`) && grid.value && props.scroller) {
    const scroller = props.scroller;
    const gridTop = grid.value.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
    scroller.scrollTop += gridTop + index * rowHeight - scroller.clientHeight / 2;
    window.value = rowWindow(
      total.value,
      rowHeight,
      scroller.clientHeight,
      Math.max(0, index * rowHeight - scroller.clientHeight / 2),
    );
    await nextTick();
  }
  if (props.page !== page || focused.value !== index) return;
  const row = grid.value?.querySelector<HTMLElement>(`[data-row-index="${index}"]`);
  row?.focus({ preventScroll: true });
  row?.scrollIntoView({ block: 'nearest' });
}
</script>

<template>
  <section
    class="voice-songs-card"
    data-voice-song-card
    data-non-selectable
    :aria-labelledby="title"
  >
    <h2 :id="title">
      包含歌曲 <small>{{ total }}</small>
    </h2>
    <div
      ref="grid"
      role="grid"
      :aria-labelledby="title"
      :aria-rowcount="total"
      aria-colcount="1"
      aria-multiselectable="true"
      @keydown="moveFocus"
    >
      <div aria-hidden="true" :style="{ height: `${window.start * rowHeight}px` }" />
      <VoiceSongRow
        v-for="{ index, track, rights } in visibleRows"
        :key="index"
        :track="track"
        :index="index"
        :selected="selection.indices.has(index)"
        :focused="
          focused === index ||
          (focused < window.start && index === window.start) ||
          (focused >= window.end && index === window.end - 1)
        "
        :playable="rights.onlineEligible && !transportBusy"
        :copyright-blocked="rights.copyrightBlocked"
        :liked="liked.has(track.id)"
        :like-busy="busyLikes.has(track.id)"
        @select="(gesture, event) => select(index, gesture, event)"
        @focus="focused = index"
        @play="play(index, $event)"
        @like="emit('like', index)"
        @more="context(index, $event)"
        @artist="emit('artist', index, $event)"
      />
      <div aria-hidden="true" :style="{ height: `${(total - window.end) * rowHeight}px` }" />
    </div>
  </section>
</template>

<style scoped>
.voice-songs-card {
  padding: 20px 0 8px;
  border-radius: 12px;
  background: color-mix(in srgb, var(--text) 7%, transparent);
}
.voice-songs-card h2 {
  margin: 0 20px 16px;
  font-size: 20px;
  font-weight: 600;
  line-height: 24px;
}
.voice-songs-card h2 small {
  padding-inline-start: 2px;
  font-size: 13px;
  color: var(--text-secondary);
}
</style>
