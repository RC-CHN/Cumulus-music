<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { Page } from '../../composables/useMusic';
import type { PlaylistView } from '../../services/contracts';
import type { HistoryKind } from '../../services/history';
import type { MenuItem } from '../../services/menu';
import { sidebarPreferences, saveSidebarPreferences } from '../../services/sidebar';
import SidebarPlaylistGroup from './SidebarPlaylistGroup.vue';
import Icon from '../Icon.vue';
import '../../styles/sidebar.css';
const props = defineProps<{
  page: Page;
  playlists: PlaylistView[];
  historyKind: HistoryKind;
  loading: boolean;
  error: string;
  playingId?: string;
  items: (list: PlaylistView) => MenuItem[];
}>();
const emit = defineEmits<{
  navigate: [page: Page];
  ranking: [];
  create: [];
  refresh: [];
  action: [list: PlaylistView, key: string];
}>();
const scroller = ref<HTMLElement>();
const preferences = ref(sidebarPreferences());
const preferenceError = ref('');
const pending = ref(false);
let retryFocus = false;
watch(
  () => props.loading,
  async (loading, previous) => {
    if (!loading) pending.value = false;
    if (loading || !previous || props.error || !retryFocus) return;
    retryFocus = false;
    await nextTick();
    if (document.activeElement === document.body) scroller.value?.focus({ preventScroll: true });
  },
);
watch(
  () => props.playlists,
  (lists) => {
    const focused = document.activeElement as HTMLElement | null;
    const id = focused?.dataset.sidebarId;
    if (id && scroller.value?.contains(focused) && !lists.some((list) => list.id === id))
      scroller.value.focus({ preventScroll: true });
  },
);
const favorite = computed(() => props.playlists.find((list) => list.favorite));
const own = computed(() => props.playlists.filter((list) => list.owned && !list.favorite));
const collected = computed(() => props.playlists.filter((list) => !list.owned));
interface NavItem {
  title: string;
  icon: string;
  page: Page;
}
const discovery: NavItem[] = [
  { title: '会员中心', icon: 'star', page: { kind: 'member' } },
  { title: '私人漫游', icon: 'music', page: { kind: 'fm' } },
  { title: '排行榜', icon: 'list', page: { kind: 'rankings', query: { kind: 'home' } } },
  { title: '一起听', icon: 'headphones', page: { kind: 'together' } },
  { title: '每日推荐', icon: 'calendar', page: { kind: 'daily' } },
  { title: '随心听 FM', icon: 'music', page: { kind: 'scene_fm' } },
];
const personal = computed<NavItem[]>(() => [
  { title: '我的歌单', icon: 'grid', page: { kind: 'library' } },
  ...(favorite.value
    ? [
        {
          title: '我喜欢的音乐',
          icon: 'heart',
          page: { kind: 'playlist' as const, id: favorite.value.id },
        },
      ]
    : []),
  { title: '下载管理', icon: 'download', page: { kind: 'downloads' } },
  { title: '最近播放', icon: 'history', page: { kind: 'history', scope: props.historyKind } },
  { title: '本地音乐', icon: 'music', page: { kind: 'local' } },
  { title: '音乐云盘', icon: 'cloud', page: { kind: 'cloud' } },
  { title: '我的播客', icon: 'music', page: { kind: 'podcasts' } },
  { title: '收藏的 MV', icon: 'video', page: { kind: 'mvs' } },
  { title: '专辑与关注', icon: 'star', page: { kind: 'collections', scope: 'saved_albums' } },
]);
function selected(target: Page) {
  if (target.kind === 'playlist')
    return props.page.kind === 'playlist' && props.page.id === target.id;
  return props.page.kind === target.kind || (target.kind === 'mvs' && props.page.kind === 'mv');
}
function toggle(group: 'created' | 'subscribed') {
  preferences.value[group] = !preferences.value[group];
  preferenceError.value = '';
  try {
    saveSidebarPreferences(preferences.value);
  } catch {
    preferenceError.value = '折叠状态未能保存，下次启动可能恢复默认。';
  }
}
function refresh() {
  if (props.loading || pending.value) return;
  pending.value = true;
  retryFocus = !!document.activeElement?.closest('.sidebar-status');
  emit('refresh');
}
</script>
<template>
  <aside class="sidebar" aria-label="主导航">
    <button class="brand" @click="emit('navigate', { kind: 'daily' })">
      <span class="brand-mark">C</span><span>Cumulus<small>云间音乐</small></span>
    </button>
    <div ref="scroller" class="sidebar-scroll" tabindex="-1">
      <nav
        v-for="group in [
          { id: 'discovery', title: '', items: discovery },
          { id: 'mine', title: '我的', items: personal },
        ]"
        :key="group.id"
        class="sidebar-navigation"
        :class="{ 'nav-main': !group.title }"
        :aria-label="group.title || '发现音乐'"
      >
        <h2 v-if="group.title" class="sidebar-mine-heading">{{ group.title }}</h2>
        <button
          v-for="item in group.items"
          :key="item.title"
          class="nav-item"
          :class="{ selected: selected(item.page) }"
          :aria-current="selected(item.page) ? 'page' : undefined"
          @click="item.page.kind === 'rankings' ? emit('ranking') : emit('navigate', item.page)"
        >
          <Icon :name="item.icon" /><span>{{ item.title }}</span>
        </button>
      </nav>
      <div class="sidebar-playlists">
        <p v-if="preferenceError" class="sidebar-status" role="status">{{ preferenceError }}</p>
        <div v-if="error" class="sidebar-status" role="alert">
          <p>{{ error }}</p>
          <button class="text-button" :aria-disabled="loading" @click="refresh">
            {{ loading ? '正在重试…' : '重新加载歌单' }}
          </button>
        </div>
        <p v-else-if="loading" class="sidebar-status" role="status">正在读取歌单…</p>
        <SidebarPlaylistGroup
          group="created"
          :playlists="own"
          :expanded="preferences.created"
          :scroller="scroller"
          :selected-id="page.kind === 'playlist' ? page.id : undefined"
          :playing-id="playingId"
          :items="items"
          @toggle="toggle('created')"
          @create="emit('create')"
          @open="emit('navigate', { kind: 'playlist', id: $event.id })"
          @action="(list, key) => emit('action', list, key)"
        />
        <SidebarPlaylistGroup
          v-if="collected.length"
          group="subscribed"
          :playlists="collected"
          :expanded="preferences.subscribed"
          :scroller="scroller"
          :selected-id="page.kind === 'playlist' ? page.id : undefined"
          :playing-id="playingId"
          :items="items"
          @toggle="toggle('subscribed')"
          @open="emit('navigate', { kind: 'playlist', id: $event.id })"
          @action="(list, key) => emit('action', list, key)"
        />
      </div>
    </div>
    <div class="sidebar-caption">Cumulus 社区客户端</div>
  </aside>
</template>
