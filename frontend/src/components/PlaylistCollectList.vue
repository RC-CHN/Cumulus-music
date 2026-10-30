<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import type { PlaylistView } from '../services/contracts';
import { useVirtualRows } from '../composables/useVirtualRows';
import Icon from './Icon.vue';

const props = defineProps<{
  playlists: PlaylistView[];
  disabled: boolean;
  selectedId: string;
  scrollTop: number;
  loading: boolean;
}>();
const emit = defineEmits<{ select: [id: string]; create: []; scroll: [top: number] }>();
const { container, start, end, onScroll } = useVirtualRows(() => props.playlists.length + 1, 64, 5);
const entries = computed(() => [null, ...props.playlists]);
const active = ref(0);
const broken = ref(new Set<string>());
onMounted(() => {
  if (container.value) {
    container.value.scrollTop = props.scrollTop;
    active.value = Math.min(props.playlists.length, Math.floor(props.scrollTop / 64));
    onScroll();
  }
});
function scroll() {
  emit('scroll', container.value?.scrollTop ?? 0);
  // Preserve a keyboard entry point when passive scrolling unmounts its row.
  const focused = container.value?.querySelector(':focus');
  if (focused && focused !== container.value) {
    const top = active.value * 64;
    const host = container.value!;
    if (top + 64 < host.scrollTop || top > host.scrollTop + host.clientHeight)
      host.focus({ preventScroll: true });
  }
  onScroll();
}
async function move(event: KeyboardEvent) {
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) || props.disabled) return;
  event.preventDefault();
  const host = container.value;
  if (!host) return;
  const current = document.activeElement === host ? Math.floor(host.scrollTop / 64) : active.value;
  const index =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? entries.value.length - 1
        : Math.max(
            0,
            Math.min(entries.value.length - 1, current + (event.key === 'ArrowDown' ? 1 : -1)),
          );
  active.value = index;
  if (index * 64 < host.scrollTop) host.scrollTop = index * 64;
  else if ((index + 1) * 64 > host.scrollTop + host.clientHeight)
    host.scrollTop = (index + 1) * 64 - host.clientHeight;
  onScroll();
  await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  await nextTick();
  host.querySelector<HTMLButtonElement>(`[data-index="${index}"]`)?.focus({ preventScroll: true });
}
</script>
<template>
  <div
    ref="container"
    class="collect-list"
    tabindex="0"
    aria-label="选择收藏歌单"
    @scroll.passive="scroll"
    @keydown="move"
  >
    <div :style="{ height: `${start * 64}px` }" aria-hidden="true" />
    <button
      v-for="(playlist, offset) in entries.slice(start, end)"
      :key="playlist?.id ?? 'create'"
      class="collect-row"
      :class="{ 'collect-selected': playlist?.id === selectedId }"
      :data-playlist-id="playlist?.id"
      :data-index="start + offset"
      :tabindex="start + offset === active ? 0 : -1"
      :aria-disabled="disabled"
      :aria-label="playlist ? `收藏到：${playlist.name}` : '创建新歌单'"
      @focus="active = start + offset"
      @click="!disabled && (playlist ? emit('select', playlist.id) : emit('create'))"
    >
      <span class="collect-cover" :class="{ 'collect-add': !playlist }">
        <img
          v-if="playlist?.coverUrl && !broken.has(playlist.coverUrl)"
          :src="playlist.coverUrl"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
          @error="broken.add(playlist.coverUrl!)"
        />
        <Icon v-else :name="playlist ? 'music' : 'plus'" />
      </span>
      <span class="collect-text">
        <span class="collect-name">{{ playlist?.name ?? '创建新歌单' }}</span>
        <span v-if="playlist" class="collect-count">{{ playlist.trackCount }}首</span>
      </span>
      <Icon v-if="playlist?.id === selectedId && disabled" class="collect-working" name="refresh" />
    </button>
    <div :style="{ height: `${(entries.length - end) * 64}px` }" aria-hidden="true" />
    <p v-if="!playlists.length && !loading" class="collect-empty">还没有可收藏歌曲的自建歌单</p>
  </div>
</template>
