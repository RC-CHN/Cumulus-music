<script setup lang="ts">
import type { TrackMenuRequest } from '../../services/trackMenu';
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue';
import { advanceSearchPage, type SearchPageStep } from '../../services/searchPagination';
import type { SearchViewportMemory } from '../../services/searchViewport';
import SearchCardGrid from './SearchCardGrid.vue';
import SearchTrackCard from './SearchTrackCard.vue';
import type { AccountView, TrackView } from '../../services/contracts';
import {
  displayedSearchCategories,
  searchExplanation,
  type SearchCategory,
  type SearchChannel,
  type SearchItem,
  type SearchSection,
} from '../../services/search';
import { useLyricWindow } from '../../composables/useLyricWindow';
import { SEARCH_PLAYBACK_ROW_LIMIT, useSearchResults } from '../../composables/useSearchResults';
import { useMotion } from '../../composables/useMotion';
import { createPageTransition } from '../../services/pageTransition';
import { useCore } from '../../services/core';
import TrackTable from '../TrackTable.vue';
import TrackRow from '../TrackRow.vue';
import SearchLyricExtra from './SearchLyricExtra.vue';
import { waitForViewportFrames } from '../../services/viewportLayout';
import Icon from '../Icon.vue';
import SearchResultCard from './SearchResultCard.vue';
import { searchCardIsCurrent } from '../../services/searchCards';
import type { PlayerOrigin, PlaybackStatus, SearchResultPlayback } from '../../services/player';
const motion = useMotion();
const panelTransition = createPageTransition(() => motion.reduced.value, undefined, {
  name: 'search-results',
  deadlineMs: 320,
});
onBeforeUnmount(panelTransition.dispose);
const props = defineProps<{
  account: AccountView;
  keyword: string;
  category?: SearchCategory;
  channel?: SearchChannel;
  pagination?: SearchPageStep;
  correct?: boolean;
  rewrittenKeyword?: string;
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
  navigate: [
    category: SearchCategory,
    pagination: SearchPageStep | undefined,
    correct: boolean,
    rewrittenKeyword?: string,
  ];
  search: [keyword: string];
  searchOriginal: [keyword: string];
  open: [item: SearchItem];
  play: [tracks: TrackView[], index: number, context: SearchResultPlayback];
  playTrack: [track: TrackView, toggleCurrent: boolean];
  downloadTrack: [track: TrackView];
  collect: [tracks: TrackView[]];
  more: [request: TrackMenuRequest];
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
  offset = computed(() => props.pagination?.offset ?? 0);
const nextPage = computed(() =>
  view.value?.more ? advanceSearchPage(props.pagination, view.value.nextOffset) : undefined,
);
const rewrite = computed(() =>
  props.correct === false
    ? undefined
    : (view.value?.correction ?? (category.value !== 'all' ? props.rewrittenKeyword : undefined)),
);
const correctionParts = computed(() =>
  rewrite.value && category.value === 'all' ? view.value?.correctionParts : undefined,
);
const requestKeyword = computed(() =>
  category.value !== 'all' && props.correct !== false
    ? (props.rewrittenKeyword ?? props.keyword)
    : props.keyword,
);
const matchKeyword = computed(() =>
  category.value === 'all' ? props.keyword : (rewrite.value ?? requestKeyword.value),
);
const lyrics = computed(() => category.value === 'lyrics');
const songWaterfall = computed(() => category.value === 'songs' && props.pagination === undefined);
const firstNumberedSongPage: SearchPageStep = { offset: 0, number: 1, previous: undefined };
const gridWaterfall = computed(() => ['artists', 'users', 'mvs'].includes(category.value));
const waterfall = computed(() => lyrics.value || gridWaterfall.value || songWaterfall.value);
const limit = computed(() =>
  lyrics.value || songWaterfall.value ? 10 : gridWaterfall.value ? 13 : 30,
);
const visibleTabs = computed(() => displayedSearchCategories(model.tabs.value, category.value));
const selected = computed(() => visibleTabs.value.find((c) => c.key === category.value)!);
const tracks = (section: SearchSection) =>
  section.items.flatMap((i) => (i.kind === 'song' ? [i.track] : []));
function bestMatch(section: SearchSection) {
  return ['search_block_best_match:', 'search_block_hot_result_core_resources:'].some((prefix) =>
    section.id.startsWith(prefix),
  );
}
function previewItems(section: SearchSection) {
  const items = section.items.filter((i) => i.kind !== 'song');
  if (category.value !== 'all') return items;
  if (bestMatch(section)) return items.slice(0, 2);
  return section.category && !['voices', 'all'].includes(section.category)
    ? items.slice(0, 6)
    : items;
}
function oneRowPreview(section: SearchSection) {
  return (
    category.value === 'all' &&
    !bestMatch(section) &&
    ['playlists', 'artists', 'albums', 'podcasts', 'mvs', 'users'].includes(section.category ?? '')
  );
}
function showTrackMenu(request: TrackMenuRequest) {
  emit('more', { ...request, nextOrigin: 'search' });
}
function playTrack(track: TrackView | undefined, toggleCurrent = false) {
  if (track && !track.missing) emit('playTrack', track, toggleCurrent);
}
const songs = computed(() => view.value?.sections.flatMap(tracks) ?? []);
const count = computed(() => view.value?.sections.reduce((n, s) => n + s.items.length, 0) ?? 0);
const explanation = computed(() => searchExplanation(view.value, category.value, count.value > 0));
const expandedLyrics = ref(new Set<string>());
function toggleLyric(key: string) {
  const next = new Set(expandedLyrics.value);
  if (!next.delete(key)) next.add(key);
  expandedLyrics.value = next;
  void nextTick(() => checkpoint());
}
const panel = ref<HTMLElement>(),
  tabs = ref<HTMLElement>();
const moreButton = ref<HTMLButtonElement>(),
  moreFooter = ref<HTMLElement>();
const searchSongTables = ref<InstanceType<typeof TrackTable>[]>([]);
const lyricBody = ref<HTMLElement>();
const lyricItems = computed(() =>
  lyrics.value ? (view.value?.sections[0]?.items.filter((item) => item.kind === 'song') ?? []) : [],
);
const lyricKey = (index: number, id: string) => `${index}:${id}`;
const lyricWindow = useLyricWindow(panel, lyricBody, () =>
  lyricItems.value.map((item, index) =>
    expandedLyrics.value.has(lyricKey(index, item.track.id))
      ? 90 + Math.max(3, item.excerpt.length) * 18
      : 144,
  ),
);
const renderedLyrics = computed(() =>
  lyricWindow.rendered.value.flatMap((index) => {
    const item = lyricItems.value[index];
    return item ? [{ item, index, key: lyricKey(index, item.track.id) }] : [];
  }),
);
function lyricFocus(event: FocusEvent) {
  const row = (event.target as Element | null)?.closest<HTMLElement>('[data-lyric-index]');
  if (row) lyricWindow.focus(Number(row.dataset.lyricIndex));
}
function lyricBlur(event: FocusEvent) {
  if (!(event.relatedTarget instanceof Node) || !lyricBody.value?.contains(event.relatedTarget))
    lyricWindow.focus();
}
function open(item: SearchItem) {
  if (item.kind === 'related') emit('search', item.keyword);
  else if (item.kind !== 'user' || !item.user.unavailable) emit('open', item);
}
function choose(key: SearchCategory, fromKeyboard = false) {
  turn(key);
  if (fromKeyboard)
    void nextTick(() =>
      tabs.value?.querySelector<HTMLButtonElement>(`[data-category="${key}"]`)?.focus(),
    );
}
function turn(key: SearchCategory, pagination?: SearchPageStep) {
  emit(
    'navigate',
    key,
    pagination,
    props.correct ?? true,
    key === 'all' ? undefined : rewrite.value,
  );
}
function tabKey(event: KeyboardEvent, index: number) {
  const categories = visibleTabs.value;
  let next = index;
  if (event.key === 'ArrowRight') next = (index + 1) % categories.length;
  else if (event.key === 'ArrowLeft') next = (index + categories.length - 1) % categories.length;
  else if (event.key === 'Home') next = 0;
  else if (event.key === 'End') next = categories.length - 1;
  else return;
  event.preventDefault();
  choose(categories[next]!.key, true);
}
let restoring = false,
  loadGeneration = 0,
  frame = 0;
let scrollDeadline: ReturnType<typeof setTimeout> | undefined;
let activeCategory = '';
let observer: ResizeObserver | undefined;
function trackFocusKey(row?: HTMLElement | null) {
  return row?.dataset.trackId
    ? `song-row:${row.getAttribute('aria-rowindex')}:${row.dataset.trackId}`
    : undefined;
}
function checkpoint(focus = false) {
  if (!panel.value || restoring || loading.value) return;
  const active =
    focus && document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
  const card = active?.closest<HTMLElement>('[data-result-key]');
  const row =
    activeCategory === 'songs'
      ? active?.closest<HTMLElement>('.track-row[data-track-id]')
      : undefined;
  const key = card?.dataset.resultKey ?? trackFocusKey(row);
  model.savePosition({
    top: panel.value.scrollTop,
    tracks: panel.value.querySelector('.track-scroll')?.scrollTop ?? 0,
    focus: key,
    focusAction: active?.dataset.resultAction,
    expandedLyrics: [...expandedLyrics.value],
    lyricAnchor: activeCategory === 'lyrics' ? lyricWindow.anchor() : undefined,
  });
}
function maybeMore(songScroll?: HTMLElement) {
  const element = songWaterfall.value ? songScroll : panel.value;
  if (songWaterfall.value && (!element || element.scrollTop <= 0)) return;
  // An append updates the model before Vue paints the new spacer. Do not
  // treat the previous page's bottom as the end of the newly loaded list.
  if (
    lyrics.value &&
    (!lyricBody.value ||
      lyricWindow.offsets.value.length !== lyricItems.value.length + 1 ||
      Math.abs(lyricBody.value.clientHeight - lyricWindow.height.value) > 1)
  )
    return;
  if (
    !restoring &&
    waterfall.value &&
    element &&
    view.value?.more &&
    !moreError.value &&
    element.scrollHeight - element.scrollTop - element.clientHeight <
      (lyrics.value || songWaterfall.value ? 60 : 180)
  )
    void model.more();
}
function scroll(event: Event) {
  checkpoint();
  if (lyrics.value) lyricWindow.schedule();
  const target = event.target;
  const songScroll =
    songWaterfall.value &&
    !restoring &&
    target instanceof HTMLElement &&
    target.classList.contains('track-scroll')
      ? target
      : undefined;
  if (songWaterfall.value && !songScroll) return;
  const generation = loadGeneration;
  if (!frame) {
    const update = () => {
      cancelAnimationFrame(frame);
      clearTimeout(scrollDeadline);
      frame = 0;
      if (generation === loadGeneration && (!songScroll || panel.value?.contains(songScroll)))
        maybeMore(songScroll);
    };
    frame = requestAnimationFrame(update);
    scrollDeadline = setTimeout(update, 64);
  }
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
  expandedLyrics.value = new Set(model.position.value.expandedLyrics ?? []);
  await nextTick();
  await waitForViewportFrames(2);
  if (
    generation !== loadGeneration ||
    !panel.value ||
    panel.value.classList.contains('search-results-leave-active')
  )
    return;
  const position = model.position.value;
  if (lyrics.value && position.lyricAnchor) lyricWindow.restore(position.lyricAnchor);
  else panel.value.scrollTop = position.top;
  if (lyrics.value && position.focus?.startsWith('lyric:')) {
    const index = Number(position.focus.split(':')[1]);
    if (Number.isInteger(index)) lyricWindow.focus(index);
  }
  if (category.value === 'songs') searchSongTables.value[0]?.restoreScroll(position.tracks);
  await waitForViewportFrames(2);
  if (generation !== loadGeneration || !panel.value) return;
  if (position.focus) {
    const card = [
      ...panel.value.querySelectorAll<HTMLElement>('[data-result-key], .track-row[data-track-id]'),
    ].find(
      (node) =>
        node.dataset.resultKey === position.focus ||
        (category.value === 'songs' && trackFocusKey(node) === position.focus),
    );
    const controls = [
      ...(card?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []),
    ];
    (
      controls.find(
        (n) => position.focusAction && n.dataset.resultAction === position.focusAction,
      ) ?? controls[0]
    )?.focus({ preventScroll: true });
  }
  restoring = false;
  maybeMore();
}
async function load(reuse = false) {
  restoring = true;
  cancelAnimationFrame(frame);
  clearTimeout(scrollDeadline);
  frame = 0;
  const generation = ++loadGeneration;
  activeCategory = category.value;
  await model.open(
    {
      keyword: requestKeyword.value,
      category: category.value,
      offset: offset.value,
      limit: limit.value,
      correct: props.correct ?? true,
      channel: props.channel,
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
watch(lyricWindow.height, () => maybeMore(), { flush: 'post' });
watch(
  () => view.value,
  () => {
    if (!restoring) void nextTick(() => maybeMore());
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
    requestKeyword.value,
    category.value,
    offset.value,
    limit.value,
    props.correct,
    props.channel,
    props.searchId,
  ],
  () => {
    if (activeCategory) checkpoint(true);
    void load(true);
  },
  { immediate: true },
);
onBeforeUnmount(() => {
  checkpoint(true);
  loadGeneration++;
  observer?.disconnect();
  cancelAnimationFrame(frame);
  clearTimeout(scrollDeadline);
  model.dispose();
});
</script>
<template>
  <section class="search-results-page">
    <div class="search-heading-copy">
      <h1
        :class="{ 'sr-only': correctionParts }"
        :aria-label="`搜索「${keyword}」`"
        :data-tooltip="correctionParts ? undefined : keyword"
      >
        {{ keyword }}
      </h1>
      <div v-if="correctionParts" class="search-correction search-correction-fragments">
        <template v-for="(part, index) in correctionParts" :key="index">
          <button
            v-if="part.highlight"
            type="button"
            class="text-button"
            :aria-label="`仍然搜索“${keyword}”`"
            @click="emit('searchOriginal', keyword)"
          >
            {{ part.text }}
          </button>
          <span v-else>{{ part.text }}</span>
        </template>
      </div>
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
          v-for="(tab, i) in visibleTabs"
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
        <span id="search-song-batch-scope" class="sr-only">
          {{ songWaterfall ? '仅限本次已加载的单曲' : '仅限当前页的单曲' }}
        </span>
        <button
          class="primary"
          aria-describedby="search-song-batch-scope"
          :disabled="!playable || !songs.some((t) => !t.missing)"
          @click="
            emit(
              'play',
              songs,
              songs.findIndex((t) => !t.missing),
              'song_tab_all',
            )
          "
        >
          <Icon name="play" />播放全部</button
        ><button
          class="secondary"
          aria-describedby="search-song-batch-scope"
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
    <div v-if="rewrite && !correctionParts" class="search-correction">
      正在显示“{{ rewrite }}”的结果
      <button class="text-button" @click="emit('searchOriginal', keyword)">
        仍然搜索“{{ keyword }}”
      </button>
    </div>
    <Transition
      :css="false"
      mode="out-in"
      @enter="panelTransition.enter"
      @leave="panelTransition.leave"
      @enter-cancelled="panelTransition.cancel"
      @leave-cancelled="panelTransition.cancel"
      @after-enter="restore"
    >
      <div
        :key="`${searchId}:${keyword}:${category}:${offset}:${limit}:${correct}`"
        id="search-result-panel"
        ref="panel"
        role="tabpanel"
        :aria-labelledby="`search-tab-${category}`"
        :aria-busy="loading"
        class="search-result-panel"
        :class="{ 'search-song-panel': category === 'songs', 'search-lyrics-panel': lyrics }"
        tabindex="0"
        @scroll.capture.passive="scroll"
      >
        <div v-if="loading" class="empty-state" role="status">正在搜索…</div>
        <div v-else-if="error" class="empty-state">
          <p role="alert">{{ error }}</p>
          <button class="secondary" @click="load()">重新加载</button>
        </div>
        <template v-else-if="view">
          <p v-if="explanation.notice" class="search-result-notice" role="status">
            {{ explanation.notice }}
          </p>
          <div v-if="explanation.lines.length" class="search-result-notice" role="status">
            <p v-for="line in explanation.lines" :key="line">{{ line }}</p>
          </div>
          <p v-if="view.unavailableCount" class="search-result-notice">
            有 {{ view.unavailableCount }} 项结果暂无法展示。
          </p>
          <div v-if="!count" class="empty-state">
            <Icon name="search" />
            <p>
              {{
                explanation.emptyTitle ||
                `没有找到相关${selected.title === '综合' ? '内容' : selected.title}`
              }}
            </p>
            <small>试试其他关键词，或切换分类</small>
          </div>
          <section
            v-for="section in view.sections"
            :key="section.id"
            class="search-result-section"
            :class="{
              'search-result-tracks': section.items.some((i) => i.kind === 'song'),
              'search-best-section': bestMatch(section),
            }"
          >
            <header v-if="category === 'all' && !bestMatch(section)">
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
                    'comprehensive_all',
                  )
                "
              >
                <Icon name="play" />
              </button>
            </header>
            <div
              v-if="category === 'lyrics'"
              class="search-lyric-list"
              role="table"
              aria-label="歌词搜索结果"
              :aria-rowcount="lyricItems.length * 2 + 1"
            >
              <div class="track-columns track-header" role="row">
                <span>#</span><span role="columnheader">歌曲 / 歌手</span
                ><span role="columnheader">专辑</span><span role="columnheader">喜欢</span
                ><span role="columnheader">时长</span>
              </div>
              <div
                :ref="(element) => (lyricBody = (element as HTMLElement | null) ?? undefined)"
                class="search-lyric-window"
                role="presentation"
                :style="{ height: `${lyricWindow.height.value}px` }"
                @focusin="lyricFocus"
                @focusout="lyricBlur"
              >
                <div
                  v-for="{ item, index, key } in renderedLyrics"
                  :key="key"
                  class="search-lyric-result"
                  :class="{ 'lyric-striped': index % 2 }"
                  :data-lyric-index="index"
                  :style="{ top: `${lyricWindow.offsets.value[index]}px` }"
                  role="rowgroup"
                  :data-result-key="`lyric:${key}`"
                >
                  <TrackRow
                    :track="item.track"
                    :match-keyword="matchKeyword"
                    :index="index"
                    :aria-rowindex="index * 2 + 2"
                    :liked="liked.has(item.track.id)"
                    :busy="busyLikes.has(item.track.id)"
                    :current-id="currentId"
                    :playable="playable"
                    navigable
                    @play="(_index, gesture) => playTrack(item.track, gesture === 'double_click')"
                    @like="emit('like', $event)"
                    @more="showTrackMenu"
                    @artist="emit('artist', $event)"
                    @album="emit('album', $event)"
                  />
                  <div role="row" :aria-rowindex="index * 2 + 3">
                    <div role="cell" aria-colspan="5">
                      <SearchLyricExtra
                        :lines="item.excerpt"
                        :truncated="item.excerptTruncated"
                        :content-id="`search-lyric-${index}`"
                        :expanded="expandedLyrics.has(key)"
                        @toggle="toggleLyric(key)"
                      />
                    </div>
                  </div>
                </div>
              </div>
            </div>
            <div
              v-else-if="category === 'all' && section.items.some((i) => i.kind === 'song')"
              class="search-track-preview"
            >
              <SearchTrackCard
                v-for="(track, index) in tracks(section).slice(0, 6)"
                :key="`${track.id}:${index}`"
                :track="track"
                :match-keyword="matchKeyword"
                :playable="playable"
                :current="track.id === currentId"
                :playing="playbackState === 'playing' || playbackState === 'buffering'"
                :liked="liked.has(track.id)"
                :busy-like="busyLikes.has(track.id)"
                @play="playTrack(track, true)"
                @like="emit('like', track)"
                @more="
                  (event) =>
                    showTrackMenu({
                      track,
                      event,
                      play: () => playTrack(track),
                    })
                "
                @download="emit('downloadTrack', track)"
                @artist="emit('artist', $event)"
              />
            </div>
            <TrackTable
              v-else-if="section.items.some((i) => i.kind === 'song')"
              ref="searchSongTables"
              :tracks="tracks(section)"
              :match-keyword="matchKeyword"
              :liked="liked"
              :busy="busyLikes"
              :playable="playable"
              :current-id="currentId"
              navigable
              @play="
                (index, gesture) => playTrack(tracks(section)[index], gesture === 'double_click')
              "
              @like="emit('like', $event)"
              @more="showTrackMenu"
              @artist="emit('artist', $event)"
              @album="emit('album', $event)"
            />
            <SearchCardGrid
              v-if="section.items.some((i) => i.kind !== 'song')"
              :items="previewItems(section)"
              :scroller="panel"
              :virtual="gridWaterfall"
              :preview="oneRowPreview(section)"
              v-slot="{ item }"
            >
              <SearchResultCard
                :item="item"
                :category="category"
                :match-keyword="matchKeyword"
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
              @click="turn(category, pagination?.previous)"
            >
              上一页</button
            ><span>第 {{ pagination?.number ?? 1 }} 页</span
            ><button
              class="secondary"
              :disabled="!nextPage"
              @click="nextPage && turn(category, nextPage)"
            >
              下一页
            </button>
          </nav>
          <div
            v-if="waterfall && (count || (songWaterfall && view.more))"
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
            <button
              v-if="songWaterfall && (moreError || count >= SEARCH_PLAYBACK_ROW_LIMIT)"
              class="text-button"
              @click="turn(category, firstNumberedSongPage)"
            >
              使用分页查看
            </button>
          </div>
        </template>
      </div>
    </Transition>
  </section>
</template>
