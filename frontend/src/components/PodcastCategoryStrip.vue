<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { PodcastCatalogueModel } from '../composables/usePodcastCatalogue';
import { AUDIOBOOK_CATEGORY } from '../services/podcast';
import Icon from './Icon.vue';

const props = defineProps<{ model: PodcastCatalogueModel; updated: boolean }>();
const emit = defineEmits<{ open: [id: string]; rankings: [] }>();
const { categories, categoriesPending, categoriesError } = props.model;
const displayed = computed(() => {
  const index = categories.value.findIndex((item) => item.id === AUDIOBOOK_CATEGORY);
  if (!props.updated || index <= 1) return categories.value;
  const order = [...categories.value];
  const [book] = order.splice(index, 1);
  if (book) order.splice(1, 0, book);
  return order;
});
const strip = ref<HTMLElement>();
const width = ref(0);
let observer: ResizeObserver | undefined;
watch(
  strip,
  (node) => {
    observer?.disconnect();
    if (!node) return;
    width.value = node.clientWidth;
    observer = new ResizeObserver(() => {
      width.value = node.clientWidth;
    });
    observer.observe(node);
  },
  { flush: 'post' },
);
onBeforeUnmount(() => observer?.disconnect());
const visible = computed(() => {
  const all = Math.max(0, Math.floor((width.value + 15) / 109) - 1);
  return categories.value.length <= all
    ? all
    : Math.max(0, Math.floor((width.value - 78) / 109) - 1);
});
const firstHidden = computed(() => displayed.value[visible.value]);
</script>

<template>
  <nav ref="strip" class="podcast-category-strip" aria-label="浏览播客分类">
    <button type="button" @click="emit('rankings')">排行榜</button>
    <button
      v-for="category in displayed.slice(0, visible)"
      :key="category.id"
      type="button"
      :data-category-id="category.id"
      :title="category.label"
      @click="emit('open', category.id)"
    >
      {{ category.label }}
    </button>
    <button
      v-if="firstHidden"
      type="button"
      class="category-more"
      :data-category-id="firstHidden.id"
      aria-label="更多分类"
      @click="emit('open', firstHidden.id)"
    >
      更多<Icon name="more" />
    </button>
  </nav>
  <p v-if="categoriesPending" role="status" class="muted">正在读取播客分类…</p>
  <div v-else-if="categoriesError" role="alert" class="podcast-category-error">
    <span>{{ categoriesError }}</span>
    <button class="text-button" type="button" @click="model.loadCategories()">重试分类</button>
  </div>
</template>

<style scoped>
.podcast-category-strip {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 15px;
  margin: 0 0 18px;
}
.podcast-category-strip button {
  width: 94px;
  height: 40px;
  min-width: 0;
  padding: 0 5px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  border: 0;
  border-radius: 8px;
  background: var(--surface-raised);
  font-size: 16px;
}
.podcast-category-strip button:hover {
  background: var(--surface-hover);
}
.podcast-category-strip .category-more {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 3px;
  margin-inline-start: auto;
  width: 78px;
  border: 1px solid var(--line);
  background: transparent;
}
.category-more .icon {
  width: 16px;
  height: 16px;
}
.podcast-category-error {
  display: flex;
  align-items: center;
  gap: 8px;
}
</style>
