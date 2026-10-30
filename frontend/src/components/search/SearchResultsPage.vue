<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue';
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
}>();
const emit = defineEmits<{
  navigate: [category: SearchCategory, offset: number, correct: boolean];
  search: [keyword: string];
  open: [item: SearchItem];
  play: [tracks: TrackView[], index: number];
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
const model = useSearchResults(useCore(), () => emit('expired'));
const { view, loading, error } = model;
const category = computed(() => props.category ?? 'all'),
  offset = computed(() => props.offset ?? 0);
const limit = computed(() => (['artists', 'users'].includes(category.value) ? 13 : 30));
const selected = computed(() => searchCategories.find((c) => c.key === category.value)!);
const tracks = (section: SearchSection) =>
  section.items.flatMap((i) => (i.kind === 'song' ? [i.track] : []));
const songs = computed(() => view.value?.sections.flatMap(tracks) ?? []);
const count = computed(() => view.value?.sections.reduce((n, s) => n + s.items.length, 0) ?? 0);
const panel = ref<HTMLElement>(),
  tabs = ref<HTMLElement>();
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
async function load() {
  await model.open({
    keyword: props.keyword,
    category: category.value,
    offset: offset.value,
    limit: limit.value,
    correct: props.correct ?? true,
  });
}
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
  ],
  () => {
    user.value = undefined;
    void load();
  },
  { immediate: true },
);
onUnmounted(model.dispose);
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
    <Transition name="search-results" mode="out-in">
      <div
        :key="`${keyword}:${category}:${offset}:${correct}`"
        id="search-result-panel"
        ref="panel"
        role="tabpanel"
        :aria-labelledby="`search-tab-${category}`"
        :aria-busy="loading"
        class="search-result-panel"
        :class="{ 'search-song-panel': category === 'songs' }"
        tabindex="0"
      >
        <div v-if="loading" class="empty-state" role="status">正在搜索…</div>
        <div v-else-if="error" class="empty-state">
          <p role="alert">{{ error }}</p>
          <button class="secondary" @click="load">重新加载</button>
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
              <h2>{{ section.title }}</h2>
              <button v-if="section.category" class="text-button" @click="choose(section.category)">
                查看全部<Icon name="forward" />
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
            <TrackTable
              v-else-if="section.items.some((i) => i.kind === 'song')"
              :tracks="tracks(section)"
              :style="
                category === 'all' ? { height: `${tracks(section).length * 56 + 36}px` } : undefined
              "
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
            <div
              v-if="section.items.some((i) => i.kind !== 'song')"
              class="search-result-grid"
              :class="{
                'search-person-grid': section.items.every(
                  (i) => i.kind === 'artist' || i.kind === 'user',
                ),
                'search-related-grid': section.items.every((i) => i.kind === 'related'),
                'search-video-grid': section.items.every((i) => i.kind === 'mv'),
                'search-voice-grid': section.items.every((i) => i.kind === 'voice'),
              }"
            >
              <SearchResultCard
                v-for="(item, i) in section.items.filter((i) => i.kind !== 'song')"
                :key="i"
                :item="item"
                :playable="playable"
                :current="searchCardIsCurrent(item, playbackOrigin, currentVoiceId)"
                :playing="playbackState === 'playing' || playbackState === 'buffering'"
                @open="open"
                @play="emit('playCard', $event)"
                @comments="emit('cardComments', $event)"
                @download="emit('cardDownload', $event)"
              />
            </div>
          </section>
          <nav
            v-if="category !== 'all' && (offset > 0 || view.more)"
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
