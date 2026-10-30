<script setup lang="ts">
import { computed, nextTick, ref, watch } from 'vue';
import type { PlaylistView } from '../../services/contracts';
import type { MenuItem } from '../../services/menu';
import { libraryPage, type LibraryMode, type LibraryGroup } from '../../services/playlistLibrary';
import PlaylistCard from '../PlaylistCard.vue';
import Icon from '../Icon.vue';
const props = defineProps<{
  group: LibraryGroup;
  playlists: PlaylistView[];
  mode: LibraryMode;
  page: number;
  loading: boolean;
  failed: boolean;
  playable: boolean;
  preparing: boolean;
  currentId?: string;
  playing: boolean;
  items: (playlist: PlaylistView) => MenuItem[];
}>();
const emit = defineEmits<{
  mode: [value: LibraryMode];
  page: [value: number];
  open: [playlist: PlaylistView];
  play: [playlist: PlaylistView];
  action: [playlist: PlaylistView, key: string];
  create: [];
  browse: [];
}>();
const section = ref<HTMLElement>();
const current = computed(() => libraryPage(props.page, props.playlists.length));
const total = computed(() => Math.max(1, Math.ceil(props.playlists.length / 20)));
const shown = computed(() => props.playlists.slice((current.value - 1) * 20, current.value * 20));
const title = computed(() => (props.group === 'created' ? '我创建的歌单' : '我收藏的歌单'));
watch(current, (value) => {
  if (value !== props.page) emit('page', value);
});
async function turn(page: number) {
  if (page < 1 || page > total.value || page === current.value) return;
  emit('page', page);
  await nextTick();
  section.value?.querySelector<HTMLElement>('h2')?.focus({ preventScroll: true });
  section.value?.scrollIntoView({ block: 'start', behavior: 'auto' });
}
defineExpose({ section });
</script>
<template>
  <section
    ref="section"
    class="library-section"
    :data-group="group"
    :aria-labelledby="`library-${group}`"
  >
    <header class="library-group-heading">
      <h2 :id="`library-${group}`" tabindex="-1">
        {{ title }} <small>{{ playlists.length }}</small>
      </h2>
      <div class="library-view-switch" role="group" :aria-label="`${title}显示方式`">
        <button
          :aria-label="`${title}：卡片视图`"
          :aria-pressed="mode === 'grid'"
          @click="emit('mode', 'grid')"
        >
          <Icon name="grid" />
        </button>
        <button
          :aria-label="`${title}：列表视图`"
          :aria-pressed="mode === 'list'"
          @click="emit('mode', 'list')"
        >
          <Icon name="list" />
        </button>
      </div>
    </header>
    <Transition name="library-mode" mode="out-in">
      <div
        :key="`${mode}:${current}`"
        :class="['library-items', mode === 'grid' ? 'playlist-grid' : 'library-table']"
      >
        <div v-if="mode === 'list' && shown.length" class="library-table-head" aria-hidden="true">
          <span>#</span><span>标题</span><span>歌曲数</span
          ><span class="library-creator">创建者</span><span />
        </div>
        <PlaylistCard
          v-for="(playlist, index) in shown"
          :key="playlist.id"
          :playlist="playlist"
          :items="items(playlist)"
          :layout="mode"
          :index="(current - 1) * 20 + index + 1"
          :playable="playable && !preparing"
          :current="currentId === playlist.id"
          :playing="playing"
          @open="emit('open', playlist)"
          @play="emit('play', playlist)"
          @action="emit('action', playlist, $event)"
        />
        <div v-if="!shown.length" class="library-empty" role="status">
          <template v-if="loading"
            ><span class="spinner" />
            <p>正在读取歌单…</p></template
          >
          <p v-else-if="failed">歌单未能加载，请重试</p>
          <template v-else>
            <Icon :name="group === 'created' ? 'music' : 'heart'" />
            <p>{{ group === 'created' ? '还没有创建歌单' : '还没有收藏歌单' }}</p>
            <button
              class="text-button"
              @click="group === 'created' ? emit('create') : emit('browse')"
            >
              {{ group === 'created' ? '新建歌单' : '去发现音乐' }}
            </button>
          </template>
        </div>
      </div>
    </Transition>
    <nav v-if="total > 1" class="library-pagination" :aria-label="`${title}分页`">
      <button
        class="icon-button"
        :aria-label="`${title}上一页`"
        :disabled="current === 1"
        @click="turn(current - 1)"
      >
        <Icon name="back" />
      </button>
      <button
        v-for="n in Array.from(new Set([1, current - 1, current, current + 1, total])).filter(
          (n) => n >= 1 && n <= total,
        )"
        :key="n"
        :aria-label="`${title}第${n}页`"
        :aria-current="n === current ? 'page' : undefined"
        @click="turn(n)"
      >
        {{ n }}
      </button>
      <button
        class="icon-button"
        :aria-label="`${title}下一页`"
        :disabled="current === total"
        @click="turn(current + 1)"
      >
        <Icon name="forward" />
      </button>
    </nav>
  </section>
</template>
