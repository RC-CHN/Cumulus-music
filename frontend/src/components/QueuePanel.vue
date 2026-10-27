<script setup lang="ts">
import { computed, ref, watch, onMounted, onUnmounted } from 'vue';
import type { PlayerModel } from '../composables/usePlayer';
import { time } from '../services/player';
import Icon from './Icon.vue';
const props = defineProps<{ player: PlayerModel }>();
const { queue, view, busy, queueError } = props.player;
const scroll = ref(0),
  height = ref(400),
  container = ref<HTMLElement>();
const row = 62;
const start = computed(() => Math.max(0, Math.floor(scroll.value / row) - 4));
const end = computed(() =>
  Math.min(queue.value?.tracks.length ?? 0, Math.ceil((scroll.value + height.value) / row) + 4),
);
const visible = computed(() => queue.value?.tracks.slice(start.value, end.value) ?? []);
let observer: ResizeObserver | undefined;
let focus: HTMLElement | null = null;
onMounted(() => {
  focus = document.activeElement as HTMLElement;
  container.value?.focus();
  observer = new ResizeObserver((e) => (height.value = e[0]?.contentRect.height ?? 400));
  if (container.value) observer.observe(container.value);
});
watch(
  container,
  (element) => {
    observer?.disconnect();
    if (element) {
      observer?.observe(element);
      element.focus();
    }
  },
  { flush: 'post' },
);
watch(
  () => queue.value?.tracks.length,
  (count) => {
    const maximum = Math.max(0, (count ?? 0) * row - height.value);
    if (scroll.value > maximum) {
      scroll.value = maximum;
      container.value?.scrollTo(0, maximum);
    }
  },
);
onUnmounted(() => {
  observer?.disconnect();
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
        <small>{{ view?.queueLen ?? 0 }} 首歌曲</small>
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
    <div
      v-else
      ref="container"
      class="queue-scroll"
      tabindex="-1"
      @scroll="scroll = ($event.target as HTMLElement).scrollTop"
    >
      <div :style="{ height: `${start * row}px` }" aria-hidden="true" />
      <div
        v-for="(item, index) in visible"
        :key="item.entryId"
        class="queue-row"
        :data-entry-id="item.entryId"
        :class="{ current: item.entryId === view?.entryId }"
        :aria-current="item.entryId === view?.entryId ? 'true' : undefined"
      >
        <button
          class="queue-song"
          :disabled="busy || item.track.missing || queue.fm"
          @click="player.action({ kind: 'select', entry_id: item.entryId })"
        >
          <Icon :name="item.entryId === view?.entryId ? 'volume' : 'music'" /><span
            ><strong class="ellipsis">{{ item.track.name }}</strong
            ><small class="ellipsis">{{ item.track.artists.join(' / ') }}</small></span
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
