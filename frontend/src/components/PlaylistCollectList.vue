<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import type { PlaylistView } from '../services/contracts';
import { useVirtualRows } from '../composables/useVirtualRows';
import Icon from './Icon.vue';

const props = withDefaults(
  defineProps<{
    playlists: PlaylistView[];
    disabled: boolean;
    selectedId: string;
    scrollTop: number;
    loading: boolean;
    purpose?: 'download' | 'collect';
    includeCreate?: boolean;
    showWorking?: boolean;
    systemLiked?: boolean;
  }>(),
  { includeCreate: true, showWorking: true },
);
const emit = defineEmits<{
  select: [id: string];
  create: [];
  liked: [];
  scroll: [top: number];
  nearEnd: [];
}>();
const entries = computed(() => [
  ...(props.includeCreate ? [null] : []),
  ...(props.systemLiked ? (['liked'] as const) : []),
  ...props.playlists,
]);
const playlistRow = (row: (typeof entries.value)[number]) => (typeof row === 'object' ? row : null);
const { container, start, end, onScroll, restore } = useVirtualRows(
  () => entries.value.length,
  64,
  5,
);
const active = ref(0);
const broken = ref(new Set<string>());
onMounted(() => {
  if (container.value) {
    restore(props.scrollTop);
    active.value = Math.max(
      0,
      Math.min(entries.value.length - 1, Math.floor(props.scrollTop / 64)),
    );
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
  const host = container.value;
  if (
    host &&
    props.playlists.length &&
    host.scrollTop + host.clientHeight + 72 >= host.scrollHeight
  )
    emit('nearEnd');
}
async function move(event: KeyboardEvent) {
  if (
    !['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key) ||
    props.disabled ||
    !entries.value.length
  )
    return;
  event.preventDefault();
  const host = container.value;
  if (!host) return;
  const from = document.activeElement;
  const current = from === host ? Math.floor(host.scrollTop / 64) : active.value;
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
  restore(host.scrollTop);
  await nextTick();
  if (
    !host.isConnected ||
    (document.activeElement !== from && document.activeElement !== document.body)
  )
    return;
  host.querySelector<HTMLButtonElement>(`[data-index="${index}"]`)?.focus({ preventScroll: true });
}
</script>
<template>
  <div
    ref="container"
    class="collect-list"
    tabindex="0"
    :aria-label="purpose === 'download' ? '选择下载歌单' : '选择收藏歌单'"
    @scroll.passive="scroll"
    @keydown="move"
  >
    <div :style="{ height: `${start * 64}px` }" aria-hidden="true" />
    <button
      v-for="(row, offset) in entries.slice(start, end)"
      :key="row === 'liked' ? 'system-liked' : (row?.id ?? 'create')"
      class="collect-row"
      :class="{ 'collect-selected': playlistRow(row)?.id === selectedId }"
      :data-playlist-id="playlistRow(row)?.id"
      :data-index="start + offset"
      :data-voice-group-destination-liked="row === 'liked' ? '' : undefined"
      :tabindex="start + offset === active ? 0 : -1"
      :aria-disabled="disabled"
      :aria-label="
        row === 'liked'
          ? '我喜欢的音乐'
          : row
            ? `${purpose === 'download' ? '下载到' : '收藏到'}：${row.name}`
            : '创建新歌单'
      "
      @focus="active = start + offset"
      @click="
        !disabled &&
        (row === 'liked' ? emit('liked') : row ? emit('select', row.id) : emit('create'))
      "
    >
      <span class="collect-cover" :class="{ 'collect-add': row === null }">
        <img
          v-if="row && row !== 'liked' && row.coverUrl && !broken.has(row.coverUrl)"
          :src="row.coverUrl"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
          @error="broken.add(row.coverUrl!)"
        />
        <Icon v-else :name="row === 'liked' ? 'heart' : row ? 'music' : 'plus'" />
      </span>
      <span class="collect-text">
        <span class="collect-name">{{
          row === 'liked' ? '我喜欢的音乐' : (row?.name ?? '创建新歌单')
        }}</span>
        <span v-if="row && row !== 'liked'" class="collect-count">{{ row.trackCount }}首</span>
      </span>
      <Icon
        v-if="playlistRow(row)?.id === selectedId && disabled && showWorking"
        class="collect-working"
        name="refresh"
      />
    </button>
    <div :style="{ height: `${(entries.length - end) * 64}px` }" aria-hidden="true" />
    <p v-if="!playlists.length && !systemLiked && !loading" class="collect-empty">
      {{ purpose === 'download' ? '还没有可下载到的自建歌单' : '还没有可收藏歌曲的自建歌单' }}
    </p>
  </div>
</template>
