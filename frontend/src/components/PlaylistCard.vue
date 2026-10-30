<script setup lang="ts">
import { ref, watch } from 'vue';
import type { PlaylistView } from '../services/contracts';
import type { MenuItem } from '../services/menu';
import type { LibraryMode } from '../services/playlistLibrary';
import Icon from './Icon.vue';
import ActionMenu from './ActionMenu.vue';
const props = withDefaults(
  defineProps<{
    playlist: PlaylistView;
    items: MenuItem[];
    layout?: LibraryMode;
    index?: number;
    playable?: boolean;
    current?: boolean;
    playing?: boolean;
  }>(),
  { layout: 'grid', index: 1, playable: false, current: false, playing: false },
);
const emit = defineEmits<{ open: []; action: [key: string]; play: [] }>();
const menu = ref<InstanceType<typeof ActionMenu>>();
const failed = ref(false);
watch(
  () => props.playlist.coverUrl,
  () => {
    failed.value = false;
  },
);
function context(event: MouseEvent | KeyboardEvent) {
  if (!props.items.length) return;
  event.preventDefault();
  const host = event.currentTarget as HTMLElement;
  const trigger =
    document.activeElement instanceof HTMLElement && host.contains(document.activeElement)
      ? document.activeElement
      : host.querySelector<HTMLElement>('[data-library-open]')!;
  const r = trigger.getBoundingClientRect();
  const pointer = event instanceof MouseEvent && event.button === 2;
  void menu.value?.showAt(
    pointer ? event.clientX : r.left,
    pointer ? event.clientY : r.bottom,
    trigger,
  );
}
function keyboard(event: KeyboardEvent) {
  if (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) context(event);
}
</script>
<template>
  <article
    class="playlist-shell"
    :class="{ 'library-row': layout === 'list', 'library-current': current }"
    @contextmenu="context"
    @keydown="keyboard"
  >
    <button
      v-if="layout === 'list'"
      class="library-index"
      :aria-label="`${current && playing ? '暂停' : '播放'}歌单：${playlist.name}`"
      :disabled="!playable || !playlist.trackCount"
      @click="emit('play')"
    >
      <span>{{ index }}</span
      ><Icon :name="current && playing ? 'pause' : 'play'" />
    </button>
    <button
      class="playlist-card"
      :data-library-open="playlist.id"
      :aria-label="playlist.name"
      :data-tooltip="playlist.name"
      @click="emit('open')"
    >
      <span class="card-cover" :class="{ 'library-heart-cover': playlist.favorite }">
        <img
          v-if="playlist.coverUrl && !failed"
          :src="playlist.coverUrl"
          alt=""
          loading="lazy"
          decoding="async"
          referrerpolicy="no-referrer"
          @error="failed = true"
        />
        <Icon v-else :name="playlist.favorite ? 'heart' : playlist.private ? 'lock' : 'music'" />
        <span
          v-if="playlist.private && !playlist.favorite && playlist.coverUrl && !failed"
          class="library-private"
          aria-label="私密歌单"
          ><Icon name="lock"
        /></span>
      </span>
      <span v-if="playlist.private" class="sr-only">私密歌单</span>
      <span class="library-card-copy">
        <span class="card-title">{{ playlist.name }}</span>
        <small v-if="layout === 'grid'">{{ playlist.trackCount }} 首</small>
      </span>
    </button>
    <template v-if="layout === 'list'">
      <span class="library-track-count">{{ playlist.trackCount }}首</span>
      <span class="library-creator" :data-tooltip="playlist.creator">{{
        playlist.creator || '—'
      }}</span>
    </template>
    <button
      v-else
      class="library-card-play"
      :aria-label="`${current && playing ? '暂停' : '播放'}歌单：${playlist.name}`"
      :disabled="!playable || !playlist.trackCount"
      @click="emit('play')"
    >
      <Icon :name="current && playing ? 'pause' : 'play'" />
    </button>
    <ActionMenu
      v-if="items.length"
      ref="menu"
      class="playlist-card-menu"
      :label="`管理歌单：${playlist.name}`"
      :items="items"
      @select="emit('action', $event)"
    />
    <span v-else-if="layout === 'list'" />
  </article>
</template>
