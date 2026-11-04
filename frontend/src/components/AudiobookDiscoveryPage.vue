<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { usePageEntryFocus } from '../composables/usePageEntryFocus';
import type { AudiobookDiscoveryModel } from '../composables/useAudiobookDiscovery';
import type { SearchItem } from '../services/search';
import Icon from './Icon.vue';
import SearchCardGrid from './search/SearchCardGrid.vue';

const props = defineProps<{ model: AudiobookDiscoveryModel }>();
const emit = defineEmits<{ open: [id: string] }>();
const root = ref<HTMLElement>();
const { categories, books, subId, pending, error, complete } = props.model;
const items = computed<SearchItem[]>(() =>
  books.value.map((podcast) => ({ kind: 'podcast', podcast })),
);
usePageEntryFocus(
  root,
  () => !pending.value,
  () => true,
);
onMounted(() => void props.model.activate());

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
  if (!subId.value) void props.model.activate();
  else if (complete.value) props.model.choose(subId.value, true);
  else void props.model.load();
}
</script>

<template>
  <section
    ref="root"
    class="podcast-page audiobook-discovery"
    aria-label="有声书分类"
    @scroll.passive="scrolled"
  >
    <header class="podcast-heading"><h1>有声书</h1></header>
    <nav v-if="categories.length" class="podcast-subtabs" aria-label="有声书子分类">
      <button
        v-for="category in categories"
        :key="category.id"
        type="button"
        :class="{ active: category.id === subId }"
        :aria-pressed="category.id === subId"
        @click="choose(category.id)"
      >
        {{ category.label }}
      </button>
    </nav>
    <SearchCardGrid v-if="items.length" :items="items" :scroller="root" virtual>
      <template #default="{ item }">
        <article
          v-if="item.kind === 'podcast'"
          class="search-result-card audiobook-row"
          :data-book-id="item.podcast.id"
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
            >{{ item.podcast.creator || '有声书' }} · {{ item.podcast.programCount }} 集</small
          >
        </article>
      </template>
    </SearchCardGrid>
    <p v-if="complete && !books.length && !error" class="empty-state">该分类暂没有有声书</p>
    <footer class="audiobook-footer">
      <span v-if="pending" role="status">正在读取有声书…</span>
      <span v-else-if="error" role="alert">{{ error }}</span>
      <button
        v-if="error || (books.length && !complete)"
        class="text-button"
        type="button"
        :aria-disabled="pending"
        @click="retry"
      >
        {{ error ? '重试' : '加载更多有声书' }}
      </button>
    </footer>
  </section>
</template>

<style scoped>
.audiobook-discovery .podcast-subtabs {
  flex-wrap: wrap;
}
.audiobook-discovery :deep(.search-result-grid) {
  grid-template-columns: minmax(0, 1fr);
}
.audiobook-row {
  display: grid;
  grid-template-columns: 48px minmax(0, 1fr);
  grid-template-rows: 24px 24px;
  gap: 0 12px;
  padding: 8px 12px;
}
.audiobook-row .search-result-art {
  grid-row: 1 / 3;
  width: 48px;
  height: 48px;
  border-radius: 8px;
}
.audiobook-row .search-result-name {
  align-self: end;
  font-size: 16px;
}
.audiobook-row .search-result-sub {
  align-self: start;
}
.audiobook-footer {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 12px;
}
</style>
