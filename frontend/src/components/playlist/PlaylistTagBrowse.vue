<script setup lang="ts">
import { nextTick, onMounted, ref } from 'vue';
import Icon from '../Icon.vue';
import type { usePlaylistTagBrowse } from '../../composables/usePlaylistTagBrowse';
import type { usePlaylistTagCatalogue } from '../../composables/usePlaylistTagCatalogue';
import PlaylistCataloguePicker from './PlaylistCataloguePicker.vue';
const props = defineProps<{
  tag: string;
  model: ReturnType<typeof usePlaylistTagBrowse>;
  catalogue: ReturnType<typeof usePlaylistTagCatalogue>;
}>();
const emit = defineEmits<{ open: [id: string]; category: [tag: string] }>();
const root = ref<HTMLElement>();
onMounted(async () => {
  const origin = document.activeElement;
  await nextTick();
  if (!root.value?.isConnected || root.value.closest('[inert]')) return;
  if (document.activeElement !== document.body && document.activeElement !== origin) return;
  const card = Array.from(
    root.value.querySelectorAll<HTMLButtonElement>('[data-tag-playlist-id]'),
  ).find((button) => button.dataset.tagPlaylistId === props.model.lastOpened.value);
  (card ?? root.value.querySelector('h1'))?.focus({ preventScroll: true });
});
function open(id: string) {
  props.model.lastOpened.value = id;
  emit('open', id);
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
  >
    <header class="page-heading">
      <div>
        <h1 tabindex="-1">歌单广场</h1>
        <p class="muted">{{ tag }}</p>
      </div>
    </header>
    <PlaylistCataloguePicker :tag="tag" :model="catalogue" @choose="emit('category', $event)" />
    <p v-if="model.busy.value" role="status">正在读取歌单…</p>
    <p v-if="model.failed.value" class="error-state" role="alert">
      无法读取此分类
      <button type="button" class="text-button" @click="model.retry">重试</button>
    </p>
    <template v-if="model.items.value">
      <p v-if="!model.items.value.length" class="muted">此页暂无歌单</p>
      <ul v-else class="playlist-tag-cards">
        <li v-for="item in model.items.value" :key="item.id">
          <button
            type="button"
            :data-tag-playlist-id="item.id"
            :aria-label="item.name"
            @click="open(item.id)"
          >
            <span class="playlist-tag-art">
              <Icon name="music" />
              <img
                v-if="item.coverUrl"
                :src="item.coverUrl"
                :key="item.coverUrl"
                alt=""
                loading="lazy"
                referrerpolicy="no-referrer"
                @error="missingImage"
              />
            </span>
            <span class="ellipsis" :title="item.name">{{ item.name }}</span>
          </button>
        </li>
      </ul>
      <nav v-if="model.offset.value || model.canNext.value" aria-label="分类歌单分页">
        <button
          type="button"
          class="text-button"
          :disabled="!model.offset.value || model.busy.value"
          @click="model.previous"
        >
          上一页
        </button>
        <button
          type="button"
          class="text-button"
          :disabled="!model.canNext.value || model.busy.value"
          @click="model.next"
        >
          下一页
        </button>
      </nav>
    </template>
  </section>
</template>

<style scoped>
.playlist-tag-cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 150px), 1fr));
  gap: 18px;
  list-style: none;
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
.playlist-tag-browse nav {
  display: flex;
  justify-content: center;
}
</style>
