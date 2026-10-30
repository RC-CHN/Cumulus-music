<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from 'vue';
import type { DownloadTask } from '../../services/downloads';
import { artworkAt } from '../../services/artwork';
import { time } from '../../services/player';
import { useVirtualRows } from '../../composables/useVirtualRows';
import Icon from '../Icon.vue';
const props = defineProps<{ tasks: DownloadTask[] }>();
const emit = defineEmits<{ play: [task: DownloadTask]; manage: [task: DownloadTask] }>();
const width = ref(836),
  columns = computed(() => (width.value < 600 ? 2 : 4));
const rowHeight = computed(
  () => Math.ceil(((width.value - (columns.value - 1) * 24) / columns.value) * 0.565) + 88,
);
const total = computed(() => Math.ceil(props.tasks.length / columns.value));
const { container, start, end, onScroll } = useVirtualRows(
  () => total.value,
  () => rowHeight.value,
  2,
);
let observer: ResizeObserver | undefined;
watch(
  container,
  (element) => {
    observer?.disconnect();
    if (element) {
      width.value = element.clientWidth;
      observer = new ResizeObserver(() => {
        width.value = element.clientWidth;
      });
      observer.observe(element);
    }
  },
  { flush: 'post' },
);
onBeforeUnmount(() => observer?.disconnect());
</script>
<template>
  <div ref="container" class="download-mv-scroll" @scroll.passive="onScroll">
    <div :style="{ height: `${start * rowHeight}px` }" aria-hidden="true" />
    <div
      v-for="index in end - start"
      :key="start + index"
      class="download-mv-row"
      :style="{
        gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
        height: `${rowHeight}px`,
      }"
    >
      <article
        v-for="task in tasks.slice((start + index - 1) * columns, (start + index) * columns)"
        :key="task.id"
        class="download-mv-card"
        :data-download-mv="task.id"
      >
        <button
          class="download-mv-cover"
          :aria-label="`播放下载MV ${task.track.name}`"
          @click="emit('play', task)"
        >
          <img
            v-if="task.track.coverUrl"
            :src="artworkAt(task.track.coverUrl, 480)"
            alt=""
            loading="lazy"
            decoding="async"
            referrerpolicy="no-referrer"
          /><Icon v-else name="video" class="download-mv-placeholder" /><Icon
            name="play"
            class="download-mv-play"
          /><span class="download-mv-resolution"
            >{{ task.actualResolution ?? task.requestedResolution ?? '—' }}P</span
          ><span class="download-mv-duration">{{ time(task.track.durationMs / 1000) }}</span>
        </button>
        <div class="download-mv-info">
          <button
            class="download-mv-title ellipsis"
            :data-tooltip="task.track.name"
            @click="emit('play', task)"
          >
            {{ task.track.name }}</button
          ><span class="ellipsis muted" :data-tooltip="task.track.artists.join(' / ')">{{
            task.track.artists.join(' / ')
          }}</span
          ><button
            class="icon-button download-mv-manage"
            :aria-label="`管理下载 ${task.track.name}`"
            @click="emit('manage', task)"
          >
            <Icon name="more" />
          </button>
        </div>
      </article>
    </div>
    <div :style="{ height: `${(total - end) * rowHeight}px` }" aria-hidden="true" />
  </div>
</template>
