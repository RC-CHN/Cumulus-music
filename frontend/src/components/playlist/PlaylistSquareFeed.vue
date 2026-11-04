<script setup lang="ts">
import { onMounted, ref, nextTick } from 'vue';
import type { usePlaylistSquare } from '../../composables/usePlaylistSquare';
import PlaylistSquareGroup from './PlaylistSquareGroup.vue';

const props = defineProps<{
  model: ReturnType<typeof usePlaylistSquare>;
  playable: boolean;
  current?: string;
  playing: boolean;
}>();
const emit = defineEmits<{ open: [id: string]; play: [id: string]; settings: [] }>();
const { groups, pending, error, enabled, continuation, nextBatch, load, retry } = props.model;
const root = ref<HTMLElement>();
onMounted(async () => {
  props.model.activate();
  const opener = props.model.takeOpener();
  if (!opener) return;
  await nextTick();
  [...(root.value?.querySelectorAll<HTMLButtonElement>('[data-square-card]') ?? [])]
    .find((item) => item.dataset.squareCard === opener)
    ?.focus();
});
function open(id: string, key: string) {
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
      <PlaylistSquareGroup
        v-for="(group, index) in groups"
        :key="index"
        :group="group"
        :index="index"
        :playable="playable"
        :current="current"
        :playing="playing"
        @open="open"
        @play="emit('play', $event)"
      />
      <p v-if="pending" class="muted" role="status">正在读取推荐歌单…</p>
      <div v-else-if="error" class="empty-state" role="status">
        <p>{{ error }}</p>
        <button class="secondary" @click="retry">重新加载推荐</button>
      </div>
      <button v-else-if="continuation" class="secondary" @click="load('more')">
        {{ nextBatch ? '查看下一批推荐歌单' : '加载更多推荐歌单' }}
      </button>
      <p v-else-if="enabled && !groups.length" class="empty-state">暂无推荐歌单。</p>
    </template>
  </div>
</template>

<style scoped>
.playlist-square-refresh {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 6px;
}
</style>
