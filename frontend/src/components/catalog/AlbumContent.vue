<script setup lang="ts">
import BoundedPanelTransition from '../BoundedPanelTransition.vue';
import { computed, ref, useId } from 'vue';
import type { AccountView, TrackView } from '../../services/contracts';
import type { AlbumCard, CatalogResource } from '../../services/catalog';
import type {
  TrackCurrentToggle,
  TrackMenuRequest,
  TrackPlayGesture,
} from '../../services/trackMenu';
import type { CatalogSession } from '../../composables/useCatalogSession';
import type { AlbumTab } from '../../services/catalogNavigation';
import TrackTable from '../TrackTable.vue';
import TrackScopeSearch from '../TrackScopeSearch.vue';
import { filterTrackScope } from '../../services/trackScopeSearch';
import CommentsPage from '../CommentsPage.vue';
const props = defineProps<{
  album: AlbumCard;
  account: AccountView;
  model: CatalogSession;
  tracks: TrackView[];
  liked: Set<string>;
  busyLikes: Set<string>;
  playable: boolean;
  currentId?: string;
  currentToggle?: TrackCurrentToggle;
}>();
const emit = defineEmits<{
  play: [index: number, gesture?: TrackPlayGesture];
  toggleCurrent: [target: TrackCurrentToggle, anchor: HTMLButtonElement];
  like: [track: TrackView];
  more: [request: TrackMenuRequest];
  open: [resource: CatalogResource];
  expired: [];
  choose: [tab: AlbumTab];
  enter: [element: Element];
}>();
type Tab = AlbumTab;
const selected = props.model.albumTab;
const comments = props.model.comments;
const keyword = ref('');
const filtered = computed(() => filterTrackScope(props.tracks, keyword.value));
function play(index: number, gesture?: TrackPlayGesture) {
  const positions = filtered.value.positions;
  const sourceIndex = positions === null ? index : positions[index];
  if (sourceIndex !== undefined) emit('play', sourceIndex, gesture);
}
const tabs = computed<{ key: Tab; label: string; count?: number }[]>(() => [
  { key: 'songs', label: '歌曲', count: props.album.trackCount },
  { key: 'comments', label: '评论', count: comments.page.value?.total },
  { key: 'details', label: '专辑详情' },
]);
const id = useId();
function select(tab: Tab) {
  emit('choose', tab);
}
function keydown(event: KeyboardEvent, index: number) {
  if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const next =
    event.key === 'Home'
      ? 0
      : event.key === 'End'
        ? tabs.value.length - 1
        : (index + (event.key === 'ArrowRight' ? 1 : -1) + tabs.value.length) % tabs.value.length;
  const tab = tabs.value[next];
  select(tab.key);
  document.getElementById(`${id}-${tab.key}`)?.focus();
}
</script>
<template>
  <div class="album-content">
    <div class="album-tabbar">
      <nav class="catalog-tabs album-tabs" role="tablist" aria-label="专辑内容">
        <button
          v-for="(tab, index) in tabs"
          :id="`${id}-${tab.key}`"
          :key="tab.key"
          role="tab"
          :aria-selected="selected === tab.key"
          :aria-controls="`${id}-panel`"
          :tabindex="selected === tab.key ? 0 : -1"
          @click="select(tab.key)"
          @keydown="keydown($event, index)"
        >
          {{ tab.label }}<small v-if="tab.count != null">{{ tab.count }}</small>
        </button>
      </nav>
      <TrackScopeSearch
        v-if="selected === 'songs'"
        v-model="keyword"
        class="album-track-search"
        label="搜索专辑歌曲"
        clear-label="清空专辑搜索"
      />
    </div>
    <BoundedPanelTransition
      name="catalog-tab"
      @enter="emit('enter', $event)"
      @before-leave="($event as HTMLElement).inert = true"
      @before-enter="($event as HTMLElement).inert = false"
    >
      <KeepAlive include="CommentsPage">
        <CommentsPage
          v-if="selected === 'comments'"
          :id="`${id}-panel`"
          key="comments"
          embedded
          :model="comments"
          :context="model.commentContext"
          data-album-panel="comments"
          :title="album.name"
          role="tabpanel"
          :aria-labelledby="`${id}-comments`"
        />
        <article
          v-else-if="selected === 'details'"
          :id="`${id}-panel`"
          key="details"
          class="catalog-description album-description"
          data-album-panel="details"
          role="tabpanel"
          :aria-labelledby="`${id}-details`"
        >
          <h2>专辑简介</h2>
          <p>{{ album.description.trim() || '暂无专辑简介' }}</p>
        </article>
        <div
          v-else
          :id="`${id}-panel`"
          key="songs"
          class="catalog-tab-panel"
          data-album-panel="songs"
          role="tabpanel"
          :aria-labelledby="`${id}-songs`"
        >
          <p v-if="filtered.positions !== null" class="playlist-track-match" role="status">
            {{ filtered.tracks.length }} 首匹配歌曲
          </p>
          <TrackTable
            v-if="filtered.tracks.length"
            navigable
            :tracks="filtered.tracks"
            :match-keyword="filtered.positions !== null ? keyword : undefined"
            :liked="liked"
            :busy="busyLikes"
            :playable="playable"
            :current-id="currentId"
            :current-toggle="currentToggle"
            @play="play"
            @toggle-current="(target, anchor) => emit('toggleCurrent', target, anchor)"
            @like="emit('like', $event)"
            @more="emit('more', $event)"
            @artist="emit('open', { kind: 'artist', id: $event })"
            @album="emit('open', { kind: 'album', id: $event })"
          />
          <p v-else class="empty-state">
            {{ filtered.positions === null ? '暂无歌曲' : '没有找到匹配的歌曲' }}
          </p>
        </div>
      </KeepAlive>
    </BoundedPanelTransition>
  </div>
</template>
