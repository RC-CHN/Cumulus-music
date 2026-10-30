<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import type { SearchViewportMemory } from '../../services/searchViewport';
import SearchCardGrid from './SearchCardGrid.vue';
import SearchTrackCard from './SearchTrackCard.vue';
import type { AccountView, TrackView } from '../../services/contracts';
import {
  searchCategories,
  type SearchCategory,
  type SearchItem,
  type SearchSection,
} from '../../services/search';
import { useSearchResults } from '../../composables/useSearchResults';
import { useCore } from '../../services/core';
import TrackTable from '../TrackTable.vue';
import ModalTransition from '../ModalTransition.vue';
import Icon from '../Icon.vue';
import SearchResultCard from './SearchResultCard.vue';
import { searchCardIsCurrent } from '../../services/searchCards';
import type { PlayerOrigin, PlaybackStatus } from '../../services/player';
const props = defineProps<{
  account: AccountView;
  keyword: string;
  category?: SearchCategory;
  offset?: number;
  correct?: boolean;
  liked: Set<string>;
  busyLikes: Set<string>;
  playable: boolean;
  currentId?: string;
  playbackOrigin?: PlayerOrigin | null;
  playbackState?: PlaybackStatus;
  currentVoiceId?: string;
  memory: SearchViewportMemory;
  searchId?: number;
}>();
const emit = defineEmits<{
  navigate: [category: SearchCategory, offset: number, correct: boolean];
  search: [keyword: string];
  open: [item: SearchItem];
  play: [tracks: TrackView[], index: number];
  togglePlayback: [];
  downloadTrack: [track: TrackView];
  collect: [tracks: TrackView[]];
  more: [track: TrackView];
  like: [track: TrackView];
  artist: [id: string];
  album: [id: string];
  expired: [];
  playCard: [item: SearchItem];
  cardComments: [item: SearchItem];
  cardDownload: [item: SearchItem];
}>();
const model = useSearchResults(useCore(), () => emit('expired'), props.memory);
const { view, loading, error, loadingMore, moreError } = model;
const category = computed(() => props.category ?? 'all'),
  offset = computed(() => props.offset ?? 0);
const waterfall = computed(() => ['artists', 'users', 'mvs'].includes(category.value));
const limit = computed(() => (waterfall.value ? 13 : 30));
const selected = computed(() => searchCategories.find((c) => c.key === category.value)!);
const tracks = (section: SearchSection) =>
  section.items.flatMap((i) => (i.kind === 'song' ? [i.track] : []));
function previewItems(section: SearchSection) {
  const items = section.items.filter((i) => i.kind !== 'song');
  if (category.value !== 'all') return items;
  if (section.id.startsWith('search_block_best_match:')) return items.slice(0, 2);
  return section.category && !['voices', 'all'].includes(section.category)
    ? items.slice(0, 6)
    : items;
}
function oneRowPreview(section: SearchSection) {
  return (
    category.value === 'all' &&
    !section.id.startsWith('search_block_best_match:') &&
    ['playlists', 'artists', 'albums', 'podcasts', 'mvs', 'users'].includes(section.category ?? '')
  );
}
function playPreview(section: SearchSection, index: number) {
  if (
    tracks(section)[index]?.id === props.currentId &&
    ['playing', 'buffering', 'paused'].includes(props.playbackState ?? '')
  )
    emit('togglePlayback');
  else emit('play', tracks(section), index);
}
const songs = computed(() => view.value?.sections.flatMap(tracks) ?? []);
const count = computed(() => view.value?.sections.reduce((n, s) => n + s.items.length, 0) ?? 0);
const panel = ref<HTMLElement>(),
  tabs = ref<HTMLElement>();
const moreButton = ref<HTMLButtonElement>(),
  moreFooter = ref<HTMLElement>();
const user = ref<Extract<SearchItem, { kind: 'user' }>>(),
  dialog = ref<HTMLDialogElement>();
async function open(item: SearchItem) {
  if (item.kind === 'user') {
    user.value = item;
    await nextTick();
    dialog.value?.showModal();
  } else if (item.kind === 'related') emit('search', item.keyword);
  else emit('open', item);
}
function choose(key: SearchCategory, fromKeyboard = false) {
  emit('navigate', key, 0, props.correct ?? true);
  if (fromKeyboard)
    void nextTick(() =>
      tabs.value?.querySelector<HTMLButtonElement>(`[data-category="${key}"]`)?.focus(),
    );
}
function tabKey(event: KeyboardEvent, index: number) {
  let next = index;
  if (event.key === 'ArrowRight') next = (index + 1) % searchCategories.length;
  else if (event.key === 'ArrowLeft')
    next = (index + searchCategories.length - 1) % searchCategories.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = searchCategories.length - 1;
  else return;
  event.preventDefault();
  choose(searchCategories[next]!.key, true);
}
let restoring = false,
  loadGeneration = 0,
  frame = 0;
let activeCategory = '';
let observer: ResizeObserver | undefined;
function checkpoint(focus = false) {
  if (!panel.value || restoring || loading.value) return;
  const card =
    focus && document.activeElement instanceof Element
      ? document.activeElement.closest<HTMLElement>('[data-result-key]')
      : undefined;
  model.savePosition({
    top: panel.value.scrollTop,
    tracks: panel.value.querySelector('.track-scroll')?.scrollTop ?? 0,
    focus: card?.dataset.resultKey,
  });
}
function maybeMore() {
  const element = panel.value;
  if (
    !restoring &&
    waterfall.value &&
    element &&
    view.value?.more &&
    !moreError.value &&
    element.scrollHeight - element.scrollTop - element.clientHeight < 180
  )
    void model.more();
}
function scroll() {
  checkpoint();
  if (!frame)
    frame = requestAnimationFrame(() => {
      frame = 0;
      maybeMore();
    });
}
watch(
  () => view.value?.more,
  (more) => {
    if (more || !moreButton.value || document.activeElement !== moreButton.value) return;
    const generation = loadGeneration;
    void nextTick(() => {
      if (generation === loadGeneration && document.activeElement === document.body) {
        moreFooter.value?.focus({ preventScroll: true });
        moreFooter.value?.scrollIntoView({ block: 'nearest' });
      }
    });
  },
);
async function restore() {
  if (
    !restoring ||
    loading.value ||
    !panel.value ||
    panel.value.classList.contains('search-results-leave-active')
  )
    return;
  const generation = loadGeneration;
  await nextTick();
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  if (
    generation !== loadGeneration ||
    !panel.value ||
    panel.value.classList.contains('search-results-leave-active')
  )
    return;
  const position = model.position.value;
  panel.value.scrollTop = position.top;
  const inner = panel.value.querySelector('.track-scroll');
  if (inner) inner.scrollTop = position.tracks;
  await new Promise<void>((resolve) =>
    requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
  );
  if (generation !== loadGeneration || !panel.value) return;
  if (position.focus) {
    const card = [...panel.value.querySelectorAll<HTMLElement>('[data-result-key]')].find(
      (n) => n.dataset.resultKey === position.focus,
    );
    card?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus({ preventScroll: true });
  }
  restoring = false;
  maybeMore();
}
async function load(reuse = false) {
  restoring = true;
  const generation = ++loadGeneration;
  activeCategory = category.value;
  await model.open(
    {
      keyword: props.keyword,
      category: category.value,
      offset: offset.value,
      limit: limit.value,
      correct: props.correct ?? true,
    },
    reuse,
    props.searchId ?? 0,
  );
  if (generation === loadGeneration) void restore();
}
watch(
  panel,
  (element) => {
    observer?.disconnect();
    if (element) {
      observer = new ResizeObserver(() => maybeMore());
      observer.observe(element);
      void restore();
    }
  },
  { flush: 'post' },
);
watch(
  () => view.value,
  () => {
    if (!restoring) void nextTick(maybeMore);
  },
);
watch(
  () => [props.account.epoch, props.account.status],
  () => model.connect(props.account),
  { immediate: true },
);
watch(
  () => [
    props.account.epoch,
    props.account.status,
    props.keyword,
    category.value,
    offset.value,
    props.correct,
    props.searchId,
  ],
  () => {
    if (activeCategory) checkpoint();
    user.value = undefined;
    void load(true);
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  checkpoint(true);
  loadGeneration++;
  observer?.disconnect();
  cancelAnimationFrame(frame);
  model.dispose();
});
</script>
<template>
  <section class="search-results-page">
    <div class="search-heading-copy">
      <h1 :aria-label="`搜索「${keyword}」`" :data-tooltip="keyword">{{ keyword }}</h1>
      <p aria-live="polite">
        {{
          loading
            ? '正在搜索…'
            : error
              ? '加载失败'
              : view?.total != null
                ? `找到 ${view.total} ${selected.unit}`
                : '综合结果'
        }}
      </p>
    </div>
    <div class="search-results-toolbar">
      <div ref="tabs" class="search-result-tabs" role="tablist" aria-label="搜索结果分类">
        <button
          v-for="(tab, i) in searchCategories"
          :id="`search-tab-${tab.key}`"
          :key="tab.key"
          role="tab"
          :aria-selected="category === tab.key"
          aria-controls="search-result-panel"
          :tabindex="category === tab.key ? 0 : -1"
          :data-category="tab.key"
          @click="choose(tab.key)"
          @keydown="tabKey($event, i)"
        >
          {{ tab.title }}
        </button>
      </div>
      <div v-if="category === 'songs'" class="search-result-actions">
        <button
          class="primary"
          :disabled="!playable || !songs.some((t) => !t.missing)"
          @click="
            emit(
              'play',
              songs,
              songs.findIndex((t) => !t.missing),
            )
          "
        >
          <Icon name="play" />播放全部</button
        ><button
          class="secondary"
          :disabled="!songs.some((t) => !t.missing)"
          @click="
            emit(
              'collect',
              songs.filter((t) => !t.missing),
            )
          "
        >
          <Icon name="plus" />收藏全部
        </button>
      </div>
    </div>
    <div v-if="view?.correction" class="search-correction">
      正在显示“{{ view.correction }}”的结果
      <button class="text-button" @click="emit('navigate', category, 0, false)">
        仍然搜索“{{ keyword }}”
      </button>
    </div>
    <Transition name="search-results" mode="out-in" @after-enter="restore">
      <div
        :key="`${searchId}:${keyword}:${category}:${offset}:${correct}`"
        id="search-result-panel"
        ref="panel"
        role="tabpanel"
        :aria-labelledby="`search-tab-${category}`"
        :aria-busy="loading"
        class="search-result-panel"
        :class="{ 'search-song-panel': category === 'songs' }"
        tabindex="0"
        @scroll.capture.passive="scroll"
      >
        <div v-if="loading" class="empty-state" role="status">正在搜索…</div>
        <div v-else-if="error" class="empty-state">
          <p role="alert">{{ error }}</p>
          <button class="secondary" @click="load()">重新加载</button>
        </div>
        <template v-else-if="view">
          <p v-if="view.notice" class="search-result-notice" role="status">{{ view.notice }}</p>
          <p v-if="view.unavailableCount" class="search-result-notice">
            有 {{ view.unavailableCount }} 项结果暂无法展示。
          </p>
          <div v-if="!count" class="empty-state">
            <Icon name="search" />
            <p>没有找到相关{{ selected.title === '综合' ? '内容' : selected.title }}</p>
            <small>试试其他关键词，或切换分类</small>
          </div>
          <section
            v-for="section in view.sections"
            :key="section.id"
            class="search-result-section"
            :class="{
              'search-result-tracks': section.items.some((i) => i.kind === 'song'),
              'search-best-section': section.id.startsWith('search_block_best_match:'),
            }"
          >
            <header v-if="category === 'all' && !section.id.startsWith('search_block_best_match:')">
              <h2>
                <button
                  v-if="section.category"
                  class="search-section-link"
                  :aria-label="`查看全部${section.title}`"
                  @click="choose(section.category)"
                >
                  {{ section.title }}<Icon name="forward" /></button
                ><template v-else>{{ section.title }}</template>
              </h2>
              <button
                v-if="section.items.some((i) => i.kind === 'song')"
                class="icon-button search-section-play"
                aria-label="播放全部搜索单曲"
                :disabled="!playable || !tracks(section).some((t) => !t.missing)"
                @click="
                  emit(
                    'play',
                    tracks(section),
                    tracks(section).findIndex((t) => !t.missing),
                  )
                "
              >
                <Icon name="play" />
              </button>
            </header>
            <div v-if="category === 'lyrics'" class="search-lyric-list">
              <article
                v-for="item in section.items"
                :key="item.kind === 'song' ? item.track.id : ''"
              >
                <template v-if="item.kind === 'song'"
                  ><button
                    class="search-lyric-title"
                    :disabled="!playable || item.track.missing"
                    @click="emit('play', [item.track], 0)"
                  >
                    <Icon name="play" />{{ item.track.name }}
                  </button>
                  <p class="muted">{{ item.track.artists.join(' / ') }} · {{ item.track.album }}</p>
                  <blockquote v-if="item.excerpt.length">
                    <p v-for="(line, i) in item.excerpt" :key="i">{{ line }}</p>
                  </blockquote>
                  <p v-else class="muted">暂无匹配片段</p>
                  <button class="text-button" @click="emit('more', item.track)">
                    更多操作
                  </button></template
                >
              </article>
            </div>
            <div
              v-else-if="category === 'all' && section.items.some((i) => i.kind === 'song')"
              class="search-track-preview"
            >
              <SearchTrackCard
                v-for="(track, index) in tracks(section).slice(0, 6)"
                :key="`${track.id}:${index}`"
                :track="track"
                :playable="playable"
                :current="track.id === currentId"
                :playing="playbackState === 'playing' || playbackState === 'buffering'"
                :liked="liked.has(track.id)"
                :busy-like="busyLikes.has(track.id)"
                @play="playPreview(section, index)"
                @like="emit('like', track)"
                @more="emit('more', track)"
                @download="emit('downloadTrack', track)"
                @artist="emit('artist', $event)"
              />
            </div>
            <TrackTable
              v-else-if="section.items.some((i) => i.kind === 'song')"
              :tracks="tracks(section)"
              :liked="liked"
              :busy="busyLikes"
              :playable="playable"
              :current-id="currentId"
              navigable
              @play="emit('play', tracks(section), $event)"
              @like="emit('like', $event)"
              @more="emit('more', $event)"
              @artist="emit('artist', $event)"
              @album="emit('album', $event)"
            />
            <SearchCardGrid
              v-if="section.items.some((i) => i.kind !== 'song')"
              :items="previewItems(section)"
              :scroller="panel"
              :virtual="waterfall"
              :preview="oneRowPreview(section)"
              v-slot="{ item }"
            >
              <SearchResultCard
                :item="item"
                :playable="playable"
                :current="searchCardIsCurrent(item, playbackOrigin, currentVoiceId)"
                :playing="playbackState === 'playing' || playbackState === 'buffering'"
                @open="open"
                @play="emit('playCard', $event)"
                @comments="emit('cardComments', $event)"
                @download="emit('cardDownload', $event)"
              />
            </SearchCardGrid>
          </section>
          <nav
            v-if="!waterfall && category !== 'all' && (offset > 0 || view.more)"
            class="search-result-pagination"
            aria-label="搜索结果分页"
          >
            <button
              class="secondary"
              :disabled="offset === 0"
              @click="emit('navigate', category, Math.max(0, offset - limit), correct ?? true)"
            >
              上一页</button
            ><span>第 {{ Math.floor(offset / limit) + 1 }} 页</span
            ><button
              class="secondary"
              :disabled="!view.more"
              @click="emit('navigate', category, view.nextOffset, correct ?? true)"
            >
              下一页
            </button>
          </nav>
          <div
            v-if="waterfall && count"
            ref="moreFooter"
            class="search-load-more"
            :aria-busy="loadingMore"
            tabindex="-1"
          >
            <span v-if="moreError" role="alert">{{ moreError }}</span>
            <button
              v-if="view.more"
              ref="moreButton"
              class="text-button"
              :aria-disabled="loadingMore"
              @click="model.more()"
            >
              {{ loadingMore ? '正在加载更多…' : moreError ? '重试加载更多' : '加载更多' }}
            </button>
            <span v-else>没有更多可显示的结果</span>
          </div>
        </template>
      </div>
    </Transition>
    <ModalTransition
      ><dialog
        v-if="user"
        ref="dialog"
        class="search-user-dialog"
        aria-labelledby="search-user-title"
        @cancel.prevent="user = undefined"
      >
        <header>
          <h2 id="search-user-title">{{ user.user.name }}</h2>
          <button class="icon-button" aria-label="关闭用户信息" @click="user = undefined">
            <Icon name="close" />
          </button>
        </header>
        <p>{{ user.user.signature || '暂无简介' }}</p>
        <p v-if="user.user.unavailable" class="muted">此账号当前不可用</p>
      </dialog></ModalTransition
    >
  </section>
</template>
