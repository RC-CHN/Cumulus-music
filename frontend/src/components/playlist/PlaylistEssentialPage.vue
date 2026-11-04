<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { usePlaylistEssential } from '../../composables/usePlaylistEssential';
import { playlistCoverCount } from '../../services/playlistHeader';
import Icon from '../Icon.vue';
const props = defineProps<{
  tag: string;
  model: ReturnType<typeof usePlaylistEssential>;
  playable: boolean;
  current?: string;
  playing: boolean;
}>();
const emit = defineEmits<{ category: [tag: string]; open: [id: string]; play: [id: string] }>();
const { tags, tagsPending, tagsError, page, pending, error, nextBatch, load, loadTags, retry } =
  props.model;
const title = computed(
  () => tags.value?.find((tag) => tag.name === props.tag)?.displayName ?? props.tag,
);
const cards = computed(
  () =>
    page.value?.items.map((item) => ({
      ...item,
      count: playlistCoverCount(item.playCount, false),
    })) ?? [],
);
const root = ref<HTMLElement>(),
  trigger = ref<HTMLButtonElement>(),
  filterPanel = ref<HTMLElement>(),
  filterHost = ref<HTMLElement>();
const more = ref<HTMLElement>();
const filterOpen = ref(false);

onMounted(async () => {
  document.addEventListener('pointerdown', pointerOutside, true);
  const id = props.model.takeOpener();
  await nextTick();
  if (!root.value?.isConnected || root.value.closest('[inert]')) return;
  const card = Array.from(
    root.value.querySelectorAll<HTMLButtonElement>('[data-essential-card]'),
  ).find((button) => button.dataset.essentialCard === id);
  if (card) card.focus();
  else root.value.querySelector('h1')?.focus({ preventScroll: true });
});
onUnmounted(() => document.removeEventListener('pointerdown', pointerOutside, true));
watch(
  more,
  (element, _, cleanup) => {
    if (!element) return;
    const observer = new IntersectionObserver(
      ([entry]) => {
        if (
          entry?.isIntersecting &&
          more.value === element &&
          !nextBatch.value &&
          (page.value?.items.length ?? 0) >= 36 &&
          !error.value
        )
          void load();
      },
      { rootMargin: '0px 0px 100px 0px' },
    );
    observer.observe(element);
    cleanup(() => observer.disconnect());
  },
  { flush: 'post' },
);
async function toggleFilters() {
  if (filterOpen.value) return closeFilters(true);
  filterOpen.value = true;
  await nextTick();
  filterPanel.value?.focus({ preventScroll: true });
}
function closeFilters(restore: boolean) {
  filterOpen.value = false;
  if (restore) trigger.value?.focus({ preventScroll: true });
}
function pointerOutside(event: PointerEvent) {
  if (filterOpen.value && event.target instanceof Node && !filterHost.value?.contains(event.target))
    closeFilters(false);
}
function retryTags() {
  filterPanel.value?.focus({ preventScroll: true });
  void loadTags();
}
function focusOutside(event: FocusEvent) {
  if (
    event.relatedTarget instanceof Node &&
    !(event.currentTarget as HTMLElement).contains(event.relatedTarget)
  )
    closeFilters(false);
}
function escape(event: KeyboardEvent) {
  if (!filterOpen.value) return;
  event.stopPropagation();
  closeFilters(true);
}
function choose(category: string) {
  closeFilters(true);
  if (category !== props.tag) emit('category', category);
}
function open(id: string) {
  props.model.remember(id);
  emit('open', id);
}
function failedImage(event: Event) {
  (event.currentTarget as HTMLImageElement).hidden = true;
}
</script>

<template>
  <section ref="root" class="playlist-essential-page">
    <header class="page-heading">
      <div>
        <h1 tabindex="-1">精品歌单</h1>
        <p class="muted playlist-essential-current">{{ title }}</p>
      </div>
      <div
        ref="filterHost"
        class="playlist-essential-filter"
        @focusout="focusOutside"
        @keydown.esc="escape"
      >
        <button
          ref="trigger"
          class="secondary"
          :aria-expanded="filterOpen"
          aria-controls="playlist-essential-categories"
          @click="toggleFilters"
        >
          更多分类 <Icon name="down" />
        </button>
        <section
          v-if="filterOpen"
          id="playlist-essential-categories"
          ref="filterPanel"
          aria-label="精品歌单分类"
          tabindex="-1"
        >
          <p v-if="tagsPending" class="muted" role="status">正在读取精品分类…</p>
          <div v-else-if="tagsError" role="status">
            <p>{{ tagsError }}</p>
            <button class="secondary" @click="retryTags">重试分类</button>
          </div>
          <template v-else-if="tags">
            <h2>全部分类</h2>
            <div class="playlist-essential-tags">
              <button
                v-for="category in tags"
                :key="category.name"
                class="secondary"
                :aria-pressed="category.name === tag"
                @click="choose(category.name)"
              >
                {{ category.displayName ?? category.name }}
              </button>
            </div>
          </template>
        </section>
      </div>
    </header>
    <div class="playlist-essential-results" :aria-busy="pending">
      <ul v-if="cards.length" class="playlist-essential-cards">
        <li v-for="item in cards" :key="item.id">
          <button :data-essential-card="item.id" :aria-label="item.name" @click="open(item.id)">
            <span class="playlist-essential-art">
              <Icon name="music" />
              <img
                v-if="item.coverUrl"
                :src="item.coverUrl"
                alt=""
                loading="lazy"
                decoding="async"
                referrerpolicy="no-referrer"
                @error="failedImage"
              />
              <small v-if="item.count" :aria-label="item.count.label">{{ item.count.text }}</small>
            </span>
            <span class="ellipsis">{{ item.name }}</span>
          </button>
          <button
            class="playlist-essential-play"
            :class="{ active: current === item.id }"
            :disabled="!playable"
            :aria-label="`${current === item.id && playing ? '暂停' : '播放'}：${item.name}`"
            @click="emit('play', item.id)"
          >
            <Icon :name="current === item.id && playing ? 'pause' : 'play'" />
          </button>
        </li>
      </ul>
      <p v-if="pending" class="muted" role="status">正在读取精品歌单…</p>
      <div v-else-if="error" class="empty-state" role="status">
        <p>{{ error }}</p>
        <button class="secondary" @click="retry">重试精品歌单</button>
      </div>
      <div v-else-if="page?.continuation" ref="more">
        <button class="secondary" @click="load()">
          {{ nextBatch ? '查看下一批精品歌单' : '加载更多精品歌单' }}
        </button>
      </div>
      <p v-else-if="page && !cards.length" class="empty-state">暂无精品歌单。</p>
    </div>
  </section>
</template>

<style scoped>
.playlist-essential-filter {
  position: relative;
}
.playlist-essential-filter .icon {
  width: 14px;
  height: 14px;
}
#playlist-essential-categories {
  position: absolute;
  z-index: 6;
  top: calc(100% + 4px);
  right: 0;
  width: min(530px, 58vw);
  max-height: min(48vh, 350px);
  overflow: auto;
  padding: 14px;
  background: var(--surface);
  border: 1px solid var(--line);
  border-radius: 12px;
}
#playlist-essential-categories h2 {
  margin: 0 0 10px;
  font-size: 15px;
}
.playlist-essential-tags,
.playlist-essential-cards {
  display: grid;
  gap: 12px;
}
.playlist-essential-tags {
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 90px), 1fr));
}
.playlist-essential-tags button {
  min-height: 32px;
  padding: 5px 4px;
  border-radius: 18px;
  overflow-wrap: anywhere;
}
.playlist-essential-tags button[aria-pressed='true'] {
  color: var(--accent);
  background: var(--surface-hover);
}
.playlist-essential-cards {
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 150px), 1fr));
  padding: 0;
  list-style: none;
}
.playlist-essential-cards li {
  position: relative;
  min-width: 0;
}
.playlist-essential-cards [data-essential-card] {
  display: grid;
  gap: 6px;
  width: 100%;
  text-align: left;
}
.playlist-essential-art {
  position: relative;
  display: grid;
  place-items: center;
  overflow: hidden;
  aspect-ratio: 1;
  border-radius: 12px;
  background: var(--surface-alt);
}
.playlist-essential-art img {
  position: absolute;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.playlist-essential-art small {
  position: absolute;
  inset: 6px 6px auto auto;
  padding: 1px 5px;
  border-radius: 6px;
  color: white;
  background: rgb(0 0 0 / 60%);
}
.playlist-essential-play {
  position: absolute;
  inset: auto 8px 32px auto;
  display: grid;
  place-items: center;
  width: 36px;
  height: 36px;
  border-radius: 50%;
  background: var(--surface-raised);
  opacity: 0;
  transition: opacity 160ms ease;
}
.playlist-essential-cards li:hover .playlist-essential-play,
.playlist-essential-cards li:focus-within .playlist-essential-play,
.playlist-essential-play.active {
  opacity: 1;
}
</style>
