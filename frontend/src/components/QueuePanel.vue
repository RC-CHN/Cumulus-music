<script setup lang="ts">
import { computed, watch, onMounted, onUnmounted } from 'vue';
import { useVirtualRows } from '../composables/useVirtualRows';
import type { PlayerModel } from '../composables/usePlayer';
import { time } from '../services/player';
import Icon from './Icon.vue';
const props = defineProps<{ player: PlayerModel }>();
const { queue, view, busy, queueError } = props.player;
const row = 62;
const { container, start, end, onScroll } = useVirtualRows(
  () => queue.value?.tracks.length ?? 0,
  row,
);
const visible = computed(() => queue.value?.tracks.slice(start.value, end.value) ?? []);
const currentEntry = computed(() => view.value?.entryId);
const queueCount = computed(() => view.value?.queueLen ?? 0);
let focus: HTMLElement | null = null;
onMounted(() => {
  focus = document.activeElement as HTMLElement;
  container.value?.focus();
});
watch(
  container,
  (element) => {
    if (element) {
      element.focus();
    }
  },
  { flush: 'post' },
);
onUnmounted(() => {
  focus?.focus();
});
function move(index: number, direction: number) {
  const ids = queue.value?.tracks.map((i) => i.entryId);
  if (!ids || !ids[index + direction]) return;
  [ids[index], ids[index + direction]] = [ids[index + direction]!, ids[index]!];
  void props.player.action({ kind: 'reorder', entry_ids: ids });
}
</script>
<template>
  <aside
    class="queue-panel"
    aria-label="当前播放队列"
    @keydown.esc.stop="player.queueOpen.value = false"
  >
    <header>
      <div>
        <h2>{{ queue?.fm ? '私人漫游队列' : '当前播放' }}</h2>
        <small>{{ queueCount }} 首歌曲</small>
      </div>
      <button
        class="text-button"
        :disabled="busy || !queue?.tracks.length || queue.fm"
        @click="player.action({ kind: 'clear' })"
      >
        清空</button
      ><button
        class="icon-button"
        aria-label="关闭播放队列"
        @click="player.queueOpen.value = false"
      >
        <Icon name="close" />
      </button>
    </header>
    <div v-if="queueError" class="error-state">
      {{ queueError }}<button @click="player.loadQueue">重试</button>
    </div>
    <div v-if="!queue" class="empty-state">正在加载队列…</div>
    <div v-else-if="!queue.tracks.length" class="empty-state">队列为空，去选一首歌吧</div>
    <div v-else ref="container" class="queue-scroll" tabindex="-1" @scroll.passive="onScroll">
      <div :style="{ height: `${start * row}px` }" aria-hidden="true" />
      <div
        v-for="(item, index) in visible"
        :key="item.entryId"
        v-memo="[
          item,
          start + index,
          currentEntry === item.entryId,
          busy,
          queue.fm,
          queue.tracks.length,
        ]"
        class="queue-row"
        :data-entry-id="item.entryId"
        :class="{ current: item.entryId === currentEntry }"
        :aria-current="item.entryId === currentEntry ? 'true' : undefined"
      >
        <button
          class="queue-song"
          :disabled="busy || item.track.missing || queue.fm"
          @click="player.action({ kind: 'select', entry_id: item.entryId })"
        >
          <Icon :name="item.entryId === currentEntry ? 'volume' : 'music'" /><span
            ><strong class="ellipsis" :title="item.track.name">{{ item.track.name }}</strong
            ><small class="ellipsis" :title="item.track.artists.join(' / ')">{{
              item.track.artists.join(' / ')
            }}</small></span
          >
        </button>
        <time>{{ time(item.track.durationMs / 1000) }}</time>
        <button
          class="icon-button"
          aria-label="上移歌曲"
          :disabled="busy || start + index === 0 || queue.fm"
          @click="move(start + index, -1)"
        >
          <Icon name="up" /></button
        ><button
          class="icon-button"
          aria-label="下移歌曲"
          :disabled="busy || start + index + 1 === queue.tracks.length || queue.fm"
          @click="move(start + index, 1)"
        >
          <Icon name="down" /></button
        ><button
          class="icon-button"
          aria-label="移出队列"
          :disabled="busy || queue.fm"
          @click="player.action({ kind: 'remove', entry_ids: [item.entryId] })"
        >
          <Icon name="close" />
        </button>
      </div>
      <div :style="{ height: `${(queue.tracks.length - end) * row}px` }" aria-hidden="true" />
    </div>
  </aside>
</template>
