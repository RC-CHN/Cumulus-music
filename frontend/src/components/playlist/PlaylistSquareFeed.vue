<script setup lang="ts">
import { computed, onMounted, ref, nextTick } from 'vue';
import { usePlaylistWaterfall } from '../../composables/usePlaylistWaterfall';
import type { usePlaylistSquare } from '../../composables/usePlaylistSquare';
import PlaylistSquareGroup from './PlaylistSquareGroup.vue';

const props = defineProps<{
  model: ReturnType<typeof usePlaylistSquare>;
  playable: boolean;
  current?: string;
  playing: boolean;
  scroller?: HTMLElement;
}>();
const emit = defineEmits<{ open: [id: string]; play: [id: string]; settings: [] }>();
const { groups, pending, error, enabled, continuation, manual, load, retry } = props.model;
const root = ref<HTMLElement>();
const cardSize = (width: number) => Math.max(122, Math.min(180, width * 0.19));
const waterfall = usePlaylistWaterfall(
  () => props.scroller,
  () => groups.value.length,
  (width) => cardSize(width) + 110,
  () =>
    !!continuation.value &&
    !pending.value &&
    !error.value &&
    !manual.value &&
    enabled.value === true,
  () => void load('more'),
);
const { list, width, total, rowHeight, range, update } = waterfall;
const visibleGroups = computed(() =>
  groups.value
    .slice(range.value.start, range.value.end)
    .map((group, offset) => ({ group, index: range.value.start + offset })),
);
onMounted(async () => {
  const origin = document.activeElement;
  props.model.activate();
  const { scroll, opener } = props.model.restore();
  await nextTick();
  update();
  await nextTick();
  if (!props.scroller?.isConnected || props.scroller.closest('[inert]')) return;
  props.scroller.scrollTop = scroll;
  update();
  if (!opener) return;
  await nextTick();
  if (document.activeElement !== document.body && document.activeElement !== origin) return;
  [...(root.value?.querySelectorAll<HTMLButtonElement>('[data-square-card]') ?? [])]
    .find((item) => item.dataset.squareCard === opener)
    ?.focus({ preventScroll: true });
});
function open(id: string, key: string) {
  props.model.saveScroll(props.scroller?.scrollTop ?? 0);
  props.model.remember(key);
  emit('open', id);
}
</script>

<template>
  <div ref="root" class="playlist-square-feed" :aria-busy="pending">
    <div class="playlist-square-refresh">
      <button class="secondary" :disabled="pending" @click="load('first')">刷新推荐</button>
    </div>
    <div v-if="enabled === false" class="empty-state">
      <p>个性化服务已关闭，仍可浏览「官方」和其他歌单分类。</p>
      <button class="secondary" @click="emit('settings')">打开个性化设置</button>
    </div>
    <template v-else>
      <div
        ref="list"
        class="playlist-square-window"
        tabindex="-1"
        :data-loaded-groups="groups.length"
        :style="{ height: total + 'px', '--playlist-group-card': cardSize(width) + 'px' }"
      >
        <PlaylistSquareGroup
          v-for="{ group, index } in visibleGroups"
          :key="index"
          :group="group"
          :index="index"
          :start="model.rail(index)"
          :data-playlist-window-row="index"
          :style="{ position: 'absolute', insetInline: '0', top: index * rowHeight + 'px' }"
          :playable="playable"
          :current="current"
          :playing="playing"
          @open="open"
          @play="emit('play', $event)"
          @scroll="model.saveRail(index, $event)"
        />
      </div>
      <p v-if="pending" class="muted" role="status">正在读取推荐歌单…</p>
      <div v-else-if="error" class="empty-state" role="status">
        <p>{{ error }}</p>
        <button class="secondary" @click="retry">重新加载推荐</button>
      </div>
      <button v-else-if="continuation" class="secondary" @click="load('more')">
        加载更多推荐歌单
      </button>
      <p v-else-if="enabled && !groups.length" class="empty-state">暂无推荐歌单。</p>
    </template>
  </div>
</template>

<style scoped>
.playlist-square-window {
  position: relative;
}
.playlist-square-refresh {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 6px;
}
</style>
