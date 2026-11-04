<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import type { PodcastCatalogueModel } from '../composables/usePodcastCatalogue';
import { AUDIOBOOK_CATEGORY } from '../services/podcast';
import type { SearchItem } from '../services/search';
import Icon from './Icon.vue';
import SearchCardGrid from './search/SearchCardGrid.vue';

const props = defineProps<{ model: PodcastCatalogueModel; parentId: string }>();
const emit = defineEmits<{ open: [id: string]; parent: [id: string] }>();
const root = ref<HTMLElement>();
const {
  categories,
  categoriesPending,
  categoriesError,
  parent,
  cards,
  subId,
  pending,
  error,
  complete,
} = props.model;
const audiobooks = computed(() => props.parentId === AUDIOBOOK_CATEGORY);
const items = computed<SearchItem[]>(() =>
  cards.value.map((podcast) => ({ kind: 'podcast', podcast })),
);
const scores = computed(() => new Map(cards.value.map((card) => [card.id, card.score])));
function rating(id: string) {
  const score = scores.value.get(id);
  return score == null ? '-' : `${score}分`;
}
usePageEntryFocus(
  root,
  () => !pending.value && !categoriesPending.value,
  () => true,
);
onMounted(() => void props.model.activate(props.parentId));

function choose(id: string) {
  if (id !== subId.value) root.value?.scrollTo({ top: 0 });
  props.model.choose(id);
}
function scrolled() {
  const pane = root.value;
  if (
    pane &&
    pane.scrollTop > 0 &&
    pane.scrollHeight - pane.clientHeight - pane.scrollTop < 200 &&
    !error.value
  )
    void props.model.load();
}
function retry() {
  if (!subId.value) void props.model.activate(props.parentId);
  else if (complete.value) props.model.choose(subId.value, true);
  else void props.model.load();
}
</script>

<template>
  <section
    ref="root"
    class="podcast-page podcast-catalogue"
    :data-parent-category="parentId"
    :aria-label="audiobooks ? '有声书分类' : '播客分类'"
    @scroll.passive="scrolled"
  >
    <header class="podcast-heading">
      <h1>{{ parent?.label ?? (audiobooks ? '有声书' : '播客分类') }}</h1>
    </header>
    <nav v-if="categories.length" class="podcast-catalogue-parents" aria-label="播客主分类">
      <button
        v-for="category in categories"
        :key="category.id"
        type="button"
        :data-category-id="category.id"
        :aria-current="category.id === parentId ? 'page' : undefined"
        @click="category.id !== parentId && emit('parent', category.id)"
      >
        {{ category.label }}
      </button>
    </nav>
    <nav v-if="parent?.children.length" class="podcast-subtabs" aria-label="播客子分类">
      <button
        v-for="category in parent.children"
        :key="category.id"
        type="button"
        :class="{ active: category.id === subId }"
        :aria-pressed="category.id === subId"
        @click="choose(category.id)"
      >
        {{ category.label }}
      </button>
    </nav>
    <div v-if="items.length" class="podcast-catalogue-columns">
      <span>标题</span>
      <span>评分</span>
    </div>
    <SearchCardGrid v-if="items.length" :items="items" :scroller="root" virtual>
      <template #default="{ item }">
        <article
          v-if="item.kind === 'podcast'"
          class="search-result-card podcast-catalogue-row"
          :data-category-radio="item.podcast.id"
        >
          <button
            class="search-card-open"
            type="button"
            :aria-label="`打开 ${item.podcast.name}`"
            :disabled="item.podcast.unavailable"
            @click="emit('open', item.podcast.id)"
          />
          <div class="search-result-art">
            <img
              v-if="item.podcast.coverUrl"
              :src="item.podcast.coverUrl"
              alt=""
              loading="lazy"
              referrerpolicy="no-referrer"
            />
            <Icon v-else name="headphones" />
          </div>
          <strong class="search-result-name" :title="item.podcast.name">{{
            item.podcast.name
          }}</strong>
          <small class="search-result-sub"
            >{{ item.podcast.creator || (audiobooks ? '有声书' : '播客') }} ·
            {{ item.podcast.programCount }} 集</small
          >
          <span class="podcast-catalogue-rating" :aria-label="`评分：${rating(item.podcast.id)}`">
            {{ rating(item.podcast.id) }}
          </span>
        </article>
      </template>
    </SearchCardGrid>
    <p v-if="complete && !cards.length && !error" class="empty-state">该分类暂无内容</p>
    <footer class="podcast-catalogue-footer">
      <span v-if="pending || categoriesPending" role="status">正在读取分类…</span>
      <span v-else-if="categoriesError || error" role="alert">{{ categoriesError || error }}</span>
      <button
        v-if="categoriesError || error || (cards.length && !complete)"
        class="text-button"
        type="button"
        :aria-disabled="pending || categoriesPending"
        @click="retry"
      >
        {{ categoriesError || error ? '重试' : audiobooks ? '加载更多有声书' : '加载更多播客' }}
      </button>
    </footer>
  </section>
</template>

<style scoped>
.podcast-catalogue .podcast-subtabs {
  flex-wrap: wrap;
}
.podcast-catalogue-parents {
  display: flex;
  flex-wrap: wrap;
  gap: 15px 24px;
  margin: 7px 0 18px;
}
.podcast-catalogue-parents button {
  padding: 0;
  border: 0;
  font-size: 18px;
  line-height: 25px;
}
.podcast-catalogue-parents button[aria-current='page'] {
  color: var(--accent);
  font-weight: 600;
}
.podcast-catalogue :deep(.search-result-grid) {
  grid-template-columns: minmax(0, 1fr);
}
.podcast-catalogue-row,
.podcast-catalogue-columns {
  display: grid;
  grid-template-columns: 48px minmax(0, 1fr) 53px;
  gap: 0 12px;
  padding: 8px 12px;
}
.podcast-catalogue-columns {
  font-size: 13px;
  color: var(--muted);
}
.podcast-catalogue-columns span:first-child {
  grid-column: 2;
}
.podcast-catalogue-row {
  grid-template-rows: 24px 24px;
}
.podcast-catalogue-rating {
  grid-area: 1 / 3 / 3;
  align-self: center;
  overflow: hidden;
  color: var(--muted);
  font-variant-numeric: tabular-nums;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.podcast-catalogue-row .search-result-art {
  grid-row: 1 / 3;
  width: 48px;
  height: 48px;
  border-radius: 8px;
}
.podcast-catalogue-row .search-result-name {
  align-self: end;
  font-size: 16px;
}
.podcast-catalogue-row .search-result-sub {
  align-self: start;
}
.podcast-catalogue-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 12px;
}
</style>
