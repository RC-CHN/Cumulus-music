<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from 'vue';
import type { PlaylistView } from '../../services/contracts';
import type { MenuItem } from '../../services/menu';
import {
  saveLibraryView,
  type LibraryViewState,
  type LibraryGroup,
  type LibraryMode,
} from '../../services/playlistLibrary';
import PlaylistLibrarySection from './PlaylistLibrarySection.vue';
import Icon from '../Icon.vue';
import '../../styles/playlist-library.css';
const props = defineProps<{
  playlists: PlaylistView[];
  state: LibraryViewState;
  loading: boolean;
  error: string;
  notice: string;
  operationCount: number;
  playable: boolean;
  preparing: boolean;
  currentId?: string;
  playing: boolean;
  items: (playlist: PlaylistView) => MenuItem[];
}>();
const emit = defineEmits<{
  refresh: [];
  create: [];
  link: [];
  history: [];
  browse: [];
  open: [playlist: PlaylistView];
  play: [playlist: PlaylistView];
  action: [playlist: PlaylistView, key: string];
}>();
const scroll = ref<HTMLElement>();
const preferenceError = ref('');
const refreshButton = ref<HTMLButtonElement>();
const refreshPending = ref(false);
let retryFocus: HTMLElement | undefined;
function refresh() {
  if (props.loading || refreshPending.value) return;
  refreshPending.value = true;
  const active = document.activeElement;
  retryFocus =
    active instanceof HTMLElement && active.closest('.library-message') ? active : undefined;
  emit('refresh');
}
watch(
  () => props.loading,
  async (loading, previous) => {
    if (!loading) refreshPending.value = false;
    if (!previous || loading || props.error || !retryFocus) return;
    const before = retryFocus;
    retryFocus = undefined;
    await nextTick();
    if (
      !before.isConnected &&
      (document.activeElement === document.body || document.activeElement === before)
    )
      refreshButton.value?.focus({ preventScroll: true });
  },
);
const groups = computed(() => ({
  created: props.playlists.filter((p) => p.owned),
  subscribed: props.playlists.filter((p) => !p.owned),
}));
watch(
  () => props.playlists.length,
  async (count, previous) => {
    if (count || !previous) return;
    props.state.scrollTop = 0;
    props.state.focusId = '';
    await nextTick();
    if (scroll.value) scroll.value.scrollTop = 0;
  },
);
onMounted(async () => {
  await nextTick();
  if (scroll.value) {
    scroll.value.scrollTop = props.state.scrollTop;
    if (props.state.focusId) {
      const id = props.state.focusId;
      const button = [...scroll.value.querySelectorAll<HTMLElement>('[data-library-open]')].find(
        (n) => n.dataset.libraryOpen === id,
      );
      button?.focus({ preventScroll: true });
    }
  }
});
function mode(group: LibraryGroup, value: LibraryMode) {
  props.state[`${group}Mode`] = value;
  preferenceError.value = '';
  try {
    saveLibraryView(props.state, localStorage);
  } catch {
    preferenceError.value = '显示方式已切换，但未能保存，下次启动可能恢复默认。';
  }
}
function open(playlist: PlaylistView) {
  props.state.focusId = playlist.id;
  props.state.scrollTop = scroll.value?.scrollTop ?? 0;
  emit('open', playlist);
}
</script>
<template>
  <div class="playlist-library-page">
    <header class="page-heading">
      <h1>我的歌单</h1>
      <button
        ref="refreshButton"
        class="icon-button"
        aria-label="刷新当前页面"
        :aria-disabled="loading"
        @click="refresh"
      >
        <Icon name="refresh" />
      </button>
    </header>
    <div class="library-toolbar">
      <button class="primary" @click="emit('create')"><Icon name="plus" />新建歌单</button>
      <button class="text-button" @click="emit('link')">打开歌单链接</button>
      <button v-if="operationCount" class="text-button" @click="emit('history')">
        歌单操作记录 · {{ operationCount }}
      </button>
    </div>
    <div v-if="error" class="error-state library-message" role="alert">
      <span>{{ error }}</span>
      <button class="text-button" :aria-disabled="loading" @click="refresh">
        {{ loading ? '正在重试…' : '重试' }}
      </button>
    </div>
    <p v-else-if="loading" class="library-message muted" role="status">正在刷新歌单…</p>
    <p v-if="notice || preferenceError" class="library-message notice" role="status">
      {{ preferenceError || notice }}
    </p>
    <div
      ref="scroll"
      class="library-scroll"
      @scroll.passive="state.scrollTop = scroll?.scrollTop ?? 0"
    >
      <PlaylistLibrarySection
        v-for="group in ['created', 'subscribed'] as const"
        :key="group"
        :group="group"
        :playlists="groups[group]"
        :mode="state[`${group}Mode`]"
        :page="state[`${group}Page`]"
        :loading="loading"
        :failed="!!error"
        :items="items"
        :playable="playable"
        :preparing="preparing"
        :current-id="currentId"
        :playing="playing"
        @mode="mode(group, $event)"
        @page="state[`${group}Page`] = $event"
        @open="open"
        @play="emit('play', $event)"
        @action="(playlist, key) => emit('action', playlist, key)"
        @create="emit('create')"
        @browse="emit('browse')"
      />
    </div>
  </div>
</template>
