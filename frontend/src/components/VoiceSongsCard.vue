<script setup lang="ts">
import { computed, nextTick, ref, shallowRef, useId, watch } from 'vue';
import type { VoiceSongsView } from '../services/voiceSongs';
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
  play: [index: number, gesture: 'cover' | 'double_click'];
  like: [index: number];
  more: [index: number, event: MouseEvent | KeyboardEvent];
  artist: [index: number, id: string];
}>();
const title = useId();
const grid = ref<HTMLElement>();
const selected = ref<number>();
const focused = ref(0);
const rowHeight = 60;
const total = computed(() => props.page.tracks.length);
const window = shallowRef(rowWindow(total.value, rowHeight, 400, 0));

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
function select(index: number) {
  focused.value = selected.value = index;
}
function context(index: number, event: MouseEvent | KeyboardEvent) {
  select(index);
  emit('more', index, event);
}
async function moveFocus(event: KeyboardEvent) {
  if (
    !(event.target instanceof HTMLElement) ||
    !event.target.hasAttribute('data-voice-song-row') ||
    !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)
  )
    return;
  event.preventDefault();
  const index =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? total.value - 1
        : Math.max(
            0,
            Math.min(total.value - 1, focused.value + (event.key === 'ArrowDown' ? 1 : -1)),
          );
  select(index);
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
      @keydown="moveFocus"
    >
      <div aria-hidden="true" :style="{ height: `${window.start * rowHeight}px` }" />
      <VoiceSongRow
        v-for="{ index, track, rights } in visibleRows"
        :key="index"
        :track="track"
        :index="index"
        :selected="selected === index"
        :focused="
          focused === index ||
          (focused < window.start && index === window.start) ||
          (focused >= window.end && index === window.end - 1)
        "
        :playable="rights.onlineEligible && !transportBusy"
        :copyright-blocked="rights.copyrightBlocked"
        :liked="liked.has(track.id)"
        :like-busy="busyLikes.has(track.id)"
        @select="select(index)"
        @play="emit('play', index, $event)"
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
