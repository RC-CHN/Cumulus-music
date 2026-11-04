<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import type { useArtistGallery } from '../../composables/useArtistGallery';
import type { ArtistCard as Artist, CatalogResource } from '../../services/catalog';
import type { ArtistCardRequest } from '../../services/artistCards';
import ArtistCard from './ArtistCard.vue';

const props = defineProps<{
  model: ReturnType<typeof useArtistGallery>;
  playable: boolean;
  currentId?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [resource: CatalogResource];
  play: [artist: Artist];
  action: [request: ArtistCardRequest];
}>();
const { filter, page, pending, error, load, select } = props.model;
const surface = ref<HTMLElement>();
const more = ref<HTMLElement>();
const locations = [
  [-1, '全部'],
  [7, '华语'],
  [96, '欧美'],
  [8, '日本'],
  [16, '韩国'],
  [0, '其他'],
] as const;
const types = [
  [-1, '全部'],
  [1, '男歌手'],
  [2, '女歌手'],
  [3, '乐队组合'],
] as const;
const initials = [
  [-1, '热门'],
  ...Array.from(
    { length: 26 },
    (_, index) => [65 + index, String.fromCharCode(65 + index)] as const,
  ),
  [0, '#'],
] as const;
onMounted(() => {
  if (page.value.nextOffset === 0 && !error.value) void load();
});
watch(
  more,
  (element, _, cleanup) => {
    if (!element) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting && more.value === element && !error.value) void load();
      },
      { rootMargin: '0px 0px 120px 0px' },
    );
    observer.observe(element);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);
function open(resource: CatalogResource) {
  const active = document.activeElement;
  const key = active instanceof HTMLElement ? active.dataset.catalogKey : undefined;
  const artist = `artist:${resource.id}`;
  props.model.rememberOpener(key === artist || key?.startsWith(artist + ':') ? key : artist);
  emit('open', resource);
}
function restore() {
  const key = props.model.takeOpener();
  if (!key) return;
  [...(surface.value?.querySelectorAll<HTMLButtonElement>('[data-catalog-key]') ?? [])]
    .find((button) => button.dataset.catalogKey === key)
    ?.focus();
}
defineExpose({ restore });
</script>

<template>
  <section ref="surface" class="artist-gallery">
    <header class="page-heading"><h1>歌手</h1></header>
    <div class="artist-gallery-filters">
      <div class="artist-gallery-pills" role="group" aria-label="地区">
        <button
          v-for="[value, label] in locations"
          :key="value"
          :aria-pressed="filter.area === value"
          @click="select({ area: value })"
        >
          {{ label }}
        </button>
      </div>
      <div class="artist-gallery-pills" role="group" aria-label="类型">
        <button
          v-for="[value, label] in types"
          :key="value"
          :aria-pressed="filter.type === value"
          @click="select({ type: value })"
        >
          {{ label }}
        </button>
      </div>
      <div class="artist-gallery-initials" role="group" aria-label="首字母">
        <button
          v-for="[value, label] in initials"
          :key="value"
          :aria-pressed="filter.initial === value"
          @click="select({ initial: value })"
        >
          {{ label }}
        </button>
      </div>
    </div>
    <div class="artist-gallery-results" :aria-busy="pending">
      <div v-if="page.artists.length" class="artist-gallery-grid">
        <ArtistCard
          v-for="artist in page.artists"
          :key="artist.id"
          :artist="artist"
          :playable="playable"
          :current="currentId === artist.id"
          :playing="playing"
          @open="open"
          @play="emit('play', $event)"
          @action="emit('action', $event)"
        />
      </div>
      <div v-if="error" class="empty-state">
        <p role="status">{{ error }}</p>
        <button class="secondary" @click="load">重新加载</button>
      </div>
      <p v-else-if="pending" class="muted" role="status">正在加载歌手…</p>
      <div v-else-if="page.nextOffset && !page.capped" ref="more" class="artist-gallery-more">
        <button class="secondary" @click="load">加载更多</button>
      </div>
      <p v-else-if="page.capped" class="muted" role="status">
        已显示前 200 条结果，可调整筛选条件。
      </p>
      <p v-else-if="page.nextOffset === null && !page.artists.length" class="empty-state">
        没有找到歌手。
      </p>
    </div>
  </section>
</template>

<style scoped>
.artist-gallery {
  min-width: 0;
}
.artist-gallery-filters {
  display: grid;
  gap: 12px;
  padding-bottom: 12px;
}
.artist-gallery-pills,
.artist-gallery-initials {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px 12px;
}
.artist-gallery-pills > button {
  min-width: 80px;
  height: 32px;
  padding: 0 12px;
  border: 1px solid var(--line);
  border-radius: 24px;
}
.artist-gallery-initials {
  gap: 4px 2px;
}
.artist-gallery-initials > button {
  min-width: 24px;
  min-height: 24px;
  padding: 0 4px;
}
.artist-gallery-initials > button:first-child {
  margin-right: 6px;
  padding-right: 10px;
  border-right: 1px solid var(--line);
}
.artist-gallery-filters > div > button[aria-pressed='true'] {
  color: var(--accent);
  background: var(--surface-hover);
}
.artist-gallery-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 132px), 1fr));
  gap: 8px;
}
.artist-gallery-more {
  padding: 12px 0;
  text-align: center;
}
</style>
