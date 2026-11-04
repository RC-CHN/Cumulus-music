<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue';
import type { PlaylistSquarePage } from '../../services/contracts';
import { playlistCoverCount } from '../../services/playlistHeader';
import { useMotion } from '../../composables/useMotion';
import Icon from '../Icon.vue';

const props = defineProps<{
  group: PlaylistSquarePage['groups'][number];
  index: number;
  playable: boolean;
  current?: string;
  playing: boolean;
}>();
const emit = defineEmits<{ open: [id: string, key: string]; play: [id: string] }>();
const rail = ref<HTMLElement>();
const previous = ref(false),
  next = ref(false);
const motion = useMotion();
const cards = computed(() =>
  props.group.items.map((item) => ({ ...item, count: playlistCoverCount(item.playCount, false) })),
);
const title = () => props.group.title ?? '推荐歌单';
const sync = () => {
  const element = rail.value;
  if (!element) return;
  previous.value = element.scrollLeft > 2;
  next.value = element.scrollLeft + element.clientWidth < element.scrollWidth - 2;
};
function scroll(direction: -1 | 1) {
  const element = rail.value;
  if (element)
    element.scrollBy({
      left: element.clientWidth * direction,
      behavior: motion.reduced.value ? 'instant' : 'smooth',
    });
}
let observer: ResizeObserver | undefined;
onMounted(() => {
  if (!rail.value) return;
  observer = new ResizeObserver(sync);
  observer.observe(rail.value);
  sync();
});
onUnmounted(() => observer?.disconnect());
watch(
  () => props.group,
  async () => {
    await nextTick();
    rail.value?.scrollTo({ left: 0, behavior: 'instant' });
    sync();
  },
);
function failed(event: Event) {
  (event.currentTarget as HTMLImageElement).hidden = true;
}
</script>

<template>
  <section class="playlist-square-group" :aria-label="title()">
    <header>
      <h2>{{ title() }}</h2>
      <div v-if="previous || next" class="playlist-square-group-controls">
        <button
          class="icon-button"
          :aria-label="`${title()}：向左浏览`"
          :disabled="!previous"
          @click="scroll(-1)"
        >
          <Icon name="back" />
        </button>
        <button
          class="icon-button"
          :aria-label="`${title()}：向右浏览`"
          :disabled="!next"
          @click="scroll(1)"
        >
          <Icon name="forward" />
        </button>
      </div>
    </header>
    <ul ref="rail" class="playlist-square-rail" @scroll.passive="sync">
      <li v-for="item in cards" :key="item.id">
        <button
          class="playlist-square-open"
          :data-square-card="`${index}:${item.id}`"
          :aria-label="item.name"
          @click="emit('open', item.id, `${index}:${item.id}`)"
        >
          <span class="playlist-square-art">
            <Icon name="music" />
            <img
              v-if="item.coverUrl"
              :src="item.coverUrl"
              alt=""
              loading="lazy"
              decoding="async"
              referrerpolicy="no-referrer"
              @error="failed"
            />
            <small v-if="item.count" class="playlist-square-count" :aria-label="item.count.label">
              {{ item.count.text }}
            </small>
          </span>
          <span class="ellipsis" :title="item.name">{{ item.name }}</span>
        </button>
        <button
          class="playlist-square-play"
          :class="{ active: item.id === current }"
          :aria-label="`${item.id === current && playing ? '暂停' : '播放'}：${item.name}`"
          :disabled="!playable"
          @click="emit('play', item.id)"
        >
          <Icon :name="item.id === current && playing ? 'pause' : 'play'" />
        </button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.playlist-square-group {
  margin: 0 0 16px;
}
.playlist-square-group header,
.playlist-square-group-controls {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}
.playlist-square-group h2 {
  margin: 0;
  font-size: 18px;
}
.playlist-square-rail {
  display: grid;
  grid-auto-flow: column;
  grid-auto-columns: clamp(122px, 19%, 180px);
  gap: 16px;
  overflow: auto;
  padding: 4px 4px 10px;
  list-style: none;
  scroll-snap-type: x proximity;
}
.playlist-square-rail li {
  min-width: 0;
  position: relative;
  scroll-snap-align: start;
}
.playlist-square-open {
  display: grid;
  gap: 6px;
  width: 100%;
  text-align: left;
}
.playlist-square-art {
  position: relative;
  display: grid;
  place-items: center;
  overflow: hidden;
  aspect-ratio: 1;
  border-radius: 10px;
  background: var(--surface-alt);
}
.playlist-square-art img {
  position: absolute;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.playlist-square-count {
  position: absolute;
  inset: 6px 6px auto auto;
  padding: 1px 5px;
  border-radius: 6px;
  background: rgb(0 0 0 / 58%);
  color: white;
}
.playlist-square-play {
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
.playlist-square-rail li:hover .playlist-square-play,
.playlist-square-rail li:focus-within .playlist-square-play,
.playlist-square-play.active {
  opacity: 1;
}
</style>
