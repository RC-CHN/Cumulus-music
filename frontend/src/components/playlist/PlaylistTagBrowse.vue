<script setup lang="ts">
import { computed, nextTick, onMounted, ref } from 'vue';
import { usePlaylistWaterfall } from '../../composables/usePlaylistWaterfall';
import Icon from '../Icon.vue';
import type { usePlaylistTagBrowse } from '../../composables/usePlaylistTagBrowse';
import type { usePlaylistTagCatalogue } from '../../composables/usePlaylistTagCatalogue';
import type { usePlaylistTopTags } from '../../composables/usePlaylistTopTags';
import type { usePlaylistSquare } from '../../composables/usePlaylistSquare';
import PlaylistCataloguePicker from './PlaylistCataloguePicker.vue';
import PlaylistSquareFeed from './PlaylistSquareFeed.vue';
const props = defineProps<{
  tag: string;
  model: ReturnType<typeof usePlaylistTagBrowse>;
  catalogue: ReturnType<typeof usePlaylistTagCatalogue>;
  topTags: ReturnType<typeof usePlaylistTopTags>;
  square: ReturnType<typeof usePlaylistSquare>;
  playable: boolean;
  current?: string;
  playing: boolean;
}>();
const emit = defineEmits<{
  open: [id: string];
  category: [tag: string];
  play: [id: string];
  settings: [];
  essential: [tag: string];
}>();
const root = ref<HTMLElement>();
const columnCount = (width: number) => Math.max(1, Math.floor((width + 18) / 168));
const cardCount = computed(
  () => (props.model.items.value?.length ?? 0) + (props.model.essential.value ? 1 : 0),
);
const waterfall = usePlaylistWaterfall(
  () => root.value,
  (width) => Math.ceil(cardCount.value / columnCount(width)),
  (width) => Math.max(150, (width - (columnCount(width) - 1) * 18) / columnCount(width)) + 42,
  () =>
    props.tag !== '推荐' &&
    props.model.category.value === props.tag &&
    props.model.canNext.value &&
    !props.model.busy.value &&
    !props.model.failed.value &&
    !props.model.manual.value,
  () => void props.model.load(),
);
const { list, total, rowHeight, range, width, update } = waterfall;
const columns = computed(() => columnCount(width.value));
const rows = computed(() =>
  Array.from({ length: range.value.end - range.value.start }, (_, offset) => {
    const index = range.value.start + offset;
    const first = index * columns.value;
    const promotion = props.model.essential.value;
    return {
      index,
      cards: Array.from({ length: Math.min(columns.value, cardCount.value - first) }, (_, cell) => {
        const itemIndex = first + cell;
        return promotion && itemIndex === 0
          ? ({ promo: promotion, id: 'essential' } as const)
          : ({
              item: props.model.items.value?.[itemIndex - (promotion ? 1 : 0)],
              id: itemIndex,
            } as const);
      }),
    };
  }),
);
onMounted(async () => {
  if (props.tag === '推荐') return;
  const origin = document.activeElement;
  const { scroll, opener } = props.model.restore();
  await nextTick();
  update();
  await nextTick();
  if (!root.value?.isConnected || root.value.closest('[inert]')) return;
  root.value.scrollTop = scroll;
  update();
  await nextTick();
  if (document.activeElement !== document.body && document.activeElement !== origin) return;
  const card = Array.from(
    root.value.querySelectorAll<HTMLButtonElement>('[data-tag-playlist-id]'),
  ).find((button) => button.dataset.tagPlaylistId === opener);
  const promo =
    opener === 'essential'
      ? root.value.querySelector<HTMLButtonElement>('.playlist-essential-promo')
      : null;
  (card ?? promo ?? root.value.querySelector('h1'))?.focus({ preventScroll: true });
});
function open(id: string) {
  scroll();
  props.model.remember(id);
  emit('open', id);
}
function openEssential() {
  scroll();
  props.model.remember('essential');
  emit('essential', props.tag);
}
function scroll() {
  if (!root.value) return;
  if (props.tag === '推荐') props.square.saveScroll(root.value.scrollTop);
  else props.model.saveScroll(props.tag, root.value.scrollTop);
}
function missingImage(event: Event) {
  (event.currentTarget as HTMLImageElement).hidden = true;
}
</script>

<template>
  <section
    ref="root"
    class="playlist-tag-browse"
    aria-label="歌单广场"
    :aria-busy="model.busy.value"
    @scroll.passive="scroll"
  >
    <header class="page-heading">
      <div>
        <h1 tabindex="-1">歌单广场</h1>
        <p class="muted">{{ tag }}</p>
      </div>
    </header>
    <PlaylistCataloguePicker
      :tag="tag"
      :model="catalogue"
      :top="topTags"
      @choose="emit('category', $event)"
    />
    <PlaylistSquareFeed
      v-if="tag === '推荐'"
      :model="square"
      :playable="playable"
      :current="current"
      :playing="playing"
      :scroller="root"
      @open="emit('open', $event)"
      @play="emit('play', $event)"
      @settings="emit('settings')"
    />
    <template v-else>
      <div
        v-if="model.items.value"
        ref="list"
        class="playlist-tag-window"
        tabindex="-1"
        :data-loaded-count="model.items.value.length"
        :style="{ height: total + 'px' }"
      >
        <ul
          v-for="row in rows"
          :key="row.index"
          class="playlist-tag-cards"
          :data-playlist-window-row="row.index"
          :style="{
            top: row.index * rowHeight + 'px',
            gridTemplateColumns: 'repeat(' + columns + ', minmax(0, 1fr))',
          }"
        >
          <li v-for="card in row.cards" :key="card.id">
            <template v-if="'promo' in card && card.promo">
              <button class="playlist-essential-promo" @click="openEssential">
                <span class="playlist-tag-art">
                  <Icon name="star" />
                  <img
                    v-if="card.promo.coverUrl"
                    :src="card.promo.coverUrl"
                    alt=""
                    loading="lazy"
                    referrerpolicy="no-referrer"
                    @error="missingImage"
                  />
                  <strong>精品歌单</strong>
                </span>
                <span class="ellipsis">{{ card.promo.name }}</span>
              </button>
            </template>
            <button
              v-else-if="'item' in card && card.item"
              type="button"
              :data-tag-playlist-id="card.item.id"
              :aria-label="card.item.name"
              @click="open(card.item.id)"
            >
              <span class="playlist-tag-art">
                <Icon name="music" />
                <img
                  v-if="card.item.coverUrl"
                  :src="card.item.coverUrl"
                  :key="card.item.coverUrl"
                  alt=""
                  loading="lazy"
                  referrerpolicy="no-referrer"
                  @error="missingImage"
                />
              </span>
              <span class="ellipsis" :title="card.item.name">{{ card.item.name }}</span>
            </button>
          </li>
        </ul>
      </div>
      <p v-if="model.busy.value" role="status">正在读取歌单…</p>
      <p v-else-if="model.failed.value" class="error-state" role="alert">
        无法读取此分类
        <button type="button" class="text-button" @click="model.load">重试</button>
      </p>
      <button
        v-else-if="model.canNext.value"
        class="text-button playlist-tag-more"
        @click="model.load"
      >
        加载更多歌单
      </button>
      <p v-else-if="model.items.value && !cardCount" class="muted">此分类暂无歌单</p>
    </template>
  </section>
</template>

<style scoped>
.playlist-tag-browse {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  overscroll-behavior: contain;
}
.playlist-tag-window {
  position: relative;
}
.playlist-tag-cards {
  position: absolute;
  inset-inline: 0;
  display: grid;
  gap: 18px;
  list-style: none;
  margin: 0;
  padding: 0;
}
.playlist-tag-cards li {
  min-width: 0;
}
.playlist-tag-cards button {
  width: 100%;
  display: grid;
  gap: 8px;
  text-align: left;
}
.playlist-tag-art {
  position: relative;
  display: grid;
  place-items: center;
  overflow: hidden;
  aspect-ratio: 1;
  border-radius: 12px;
  background: var(--surface-alt);
}
.playlist-tag-art img {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.playlist-essential-promo strong {
  z-index: 1;
  align-self: start;
  margin-top: 12px;
  padding: 3px 8px;
  border-radius: 12px;
  color: white;
  background: rgb(0 0 0 / 65%);
}
.playlist-tag-more {
  display: block;
  margin: 0 auto 6px;
}
</style>
