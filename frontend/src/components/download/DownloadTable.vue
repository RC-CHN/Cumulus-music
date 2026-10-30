<script setup lang="ts">
import { computed, nextTick, ref, watch, useId } from 'vue';
import type {
  DownloadTask,
  DownloadQuery,
  DownloadSort,
  DownloadControl,
} from '../../services/downloads';
import { bytesLabel, downloadStatus } from '../../services/downloads';
import { downloadDate, downloadSection, downloadTableColumns } from '../../services/downloadPage';
import { useVirtualRows } from '../../composables/useVirtualRows';
import DownloadTrackTitle from './DownloadTrackTitle.vue';
import DownloadContinuation from './DownloadContinuation.vue';
import Icon from '../Icon.vue';
import TableColumnResize from '../TableColumnResize.vue';
import { useTableColumns } from '../../composables/useTableColumns';
const props = defineProps<{
  tasks: DownloadTask[];
  growth: number[];
  total: number;
  query: DownloadQuery;
  batch: boolean;
  selected: Set<string>;
  locked: boolean;
  playable: boolean;
  currentId?: string;
  repairing: Set<string>;
  more: boolean;
  loadingMore: boolean;
  moreError: string;
}>();
const emit = defineEmits<{
  toggle: [id: string];
  all: [];
  play: [task: DownloadTask];
  manage: [task: DownloadTask];
  sort: [column: DownloadSort];
  control: [task: DownloadTask, action: DownloadControl];
  load: [];
  columns: [growth: number[]];
}>();
const active = computed(() => props.query.tab === 'active'),
  voice = computed(() => props.query.category === 'voice');
const { container, start, end, onScroll, reset } = useVirtualRows(() => props.tasks.length, 64);
const growth = computed({
  get: () => props.growth,
  set: (value: number[]) => emit('columns', value),
});
const columns = useTableColumns(
  container,
  () => downloadTableColumns(downloadSection(props.query)),
  () => props.locked,
  growth,
);
const bodyId = useId();
const rows = computed(() => props.tasks.slice(start.value, end.value));
watch(
  [end, () => props.tasks.length, () => props.more, () => props.loadingMore],
  () => {
    if (end.value >= props.tasks.length && props.more && !props.loadingMore && !props.moreError)
      emit('load');
  },
  { flush: 'post' },
);
const focused = ref(0);
const headers = computed<{ label: string; sort: DownloadSort }[]>(() => [
  { label: props.query.sort === 'artist' ? '标题 · 歌手' : '标题', sort: 'name' },
  ...(active.value
    ? []
    : [
        ...(voice.value ? [] : [{ label: '专辑', sort: 'album' as const }]),
        { label: '下载时间', sort: 'completed' as const },
        { label: '大小', sort: 'size' as const },
      ]),
]);
const sorting = (sort: DownloadSort) =>
  props.query.sort === sort || (sort === 'name' && props.query.sort === 'artist');
watch(
  [
    () => props.query.tab,
    () => props.query.category,
    () => props.query.keyword,
    () => props.query.sort,
    () => props.query.descending,
    () => props.query.offset,
  ],
  () => {
    focused.value = 0;
    reset();
  },
);
watch(
  [start, end],
  ([from, to]) => {
    const current = document.activeElement?.closest<HTMLElement>('[data-download-index]');
    if (!current || !container.value?.contains(current)) return;
    const index = Number(current.dataset.downloadIndex);
    if (index < from || index >= to) {
      focused.value = Math.min(props.tasks.length - 1, Math.ceil(container.value.scrollTop / 64));
      container.value.focus({ preventScroll: true });
    }
  },
  { flush: 'pre' },
);
async function key(event: KeyboardEvent, index: number) {
  if (event.altKey || event.ctrlKey || event.metaKey) return;
  const task = props.tasks[index];
  if (!task) return;
  if (event.key === 'Enter' || event.key === ' ') {
    event.preventDefault();
    if (props.batch) emit('toggle', task.id);
    else if (!active.value && props.playable && event.key === 'Enter') emit('play', task);
    return;
  }
  if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
  event.preventDefault();
  const next = Math.max(
    0,
    Math.min(
      props.tasks.length - 1,
      event.key === 'Home'
        ? 0
        : event.key === 'End'
          ? props.tasks.length - 1
          : index + (event.key === 'ArrowDown' ? 1 : -1),
    ),
  );
  focused.value = next;
  if (container.value) {
    const top = next * 64;
    if (top < container.value.scrollTop) container.value.scrollTop = top;
    else if (top + 64 > container.value.scrollTop + container.value.clientHeight)
      container.value.scrollTop = top + 64 - container.value.clientHeight;
    onScroll();
  }
  await new Promise<void>((r) => requestAnimationFrame(() => r()));
  await nextTick();
  container.value
    ?.querySelector<HTMLElement>(`[data-download-index="${next}"]`)
    ?.focus({ preventScroll: true });
}
</script>
<template>
  <div
    class="download-table"
    :class="{ 'active-table': active, 'voice-table': voice && !active }"
    role="table"
    :aria-rowcount="total + 1"
  >
    <div class="download-row download-table-header" role="row" :style="columns.style.value">
      <span role="columnheader"
        ><input
          v-if="batch"
          type="checkbox"
          aria-label="选择已加载的全部下载"
          :checked="selected.size === tasks.length"
          :indeterminate="selected.size > 0 && selected.size < tasks.length"
          @change="emit('all')"
        /><template v-else>#</template></span
      >
      <span
        v-for="(header, index) in headers"
        :key="header.sort"
        role="columnheader"
        :aria-sort="
          !active && sorting(header.sort)
            ? query.descending
              ? 'descending'
              : 'ascending'
            : active
              ? undefined
              : 'none'
        "
      >
        <template v-if="active">{{ header.label }}</template
        ><button
          v-else
          class="download-sort-heading"
          :data-download-sort="header.sort"
          :aria-label="`${header.label}排序`"
          @click="emit('sort', header.sort)"
        >
          {{ header.label
          }}<Icon v-if="sorting(header.sort)" :name="query.descending ? 'down' : 'up'" /></button
        ><TableColumnResize
          v-if="columns.resizable(index + 1)"
          :model="columns"
          :column="index + 1"
          :label="header.label"
          :controls="bodyId"
          :data-download-resize="index + 1"
        /> </span
      ><span v-if="active" role="columnheader">进度</span>
    </div>
    <div
      ref="container"
      :id="bodyId"
      class="download-scroll"
      tabindex="-1"
      @scroll.passive="onScroll"
      @keydown.self="key($event, focused)"
    >
      <div :style="{ height: `${start * 64}px` }" aria-hidden="true" />
      <div
        v-for="(task, i) in rows"
        :key="task.id"
        class="download-row"
        :style="columns.style.value"
        :class="{ current: task.track.id === currentId, selected: selected.has(task.id) }"
        role="row"
        :aria-rowindex="query.offset + start + i + 2"
        :aria-selected="batch ? selected.has(task.id) : undefined"
        :data-download-index="start + i"
        :data-download-task="task.id"
        :tabindex="focused === start + i ? 0 : -1"
        @focus="focused = start + i"
        @keydown.self="key($event, start + i)"
        @click="batch && emit('toggle', task.id)"
        @dblclick="!active && !batch && playable && emit('play', task)"
      >
        <span role="cell" class="download-index"
          ><input
            v-if="batch"
            type="checkbox"
            tabindex="-1"
            :aria-label="`选择 ${task.track.name}`"
            :checked="selected.has(task.id)"
            @click.stop="emit('toggle', task.id)" />
          <template v-else
            ><span class="download-index-number">{{
              String(query.offset + start + i + 1).padStart(2, '0')
            }}</span
            ><button
              v-if="!active"
              class="icon-button download-index-play"
              :disabled="!playable"
              :aria-label="`播放下载 ${task.track.name}`"
              @click.stop="emit('play', task)"
              @dblclick.stop
            >
              <Icon name="play" /></button></template
        ></span>
        <div role="cell" class="download-title-cell">
          <DownloadTrackTitle
            :task="task"
            :active="active"
            :locked="locked"
            :repairing="repairing.has(task.id)"
            @control="emit('control', task, $event)"
            @manage="emit('manage', task)"
          />
        </div>
        <div
          v-if="active"
          role="cell"
          class="download-progress"
          :class="{ failed: task.status === 'error' }"
          :data-tooltip="`${downloadStatus(task)} · ${bytesLabel(task.bytes)} / ${bytesLabel(task.total)}`"
        >
          <template v-if="task.status === 'error'"
            ><Icon name="info" /><span class="download-progress-error">{{
              downloadStatus(task)
            }}</span></template
          >
          <template v-else
            ><progress
              :value="task.progress ?? undefined"
              max="1"
              :aria-label="`${task.track.name} 下载进度`"
            /><span class="download-progress-status">{{
              task.status === 'downloading' && task.progress !== null
                ? `${Math.round(task.progress * 100)}%`
                : downloadStatus(task)
            }}</span></template
          >
        </div>
        <template v-else
          ><span v-if="!voice" role="cell" class="ellipsis" :data-tooltip="task.track.album">{{
            task.track.album || '—'
          }}</span
          ><span role="cell" class="download-time">{{ downloadDate(task.completedMs) }}</span
          ><span role="cell" class="download-size">{{ bytesLabel(task.total) }}</span></template
        >
      </div>
      <div :style="{ height: `${(tasks.length - end) * 64}px` }" aria-hidden="true" />
      <DownloadContinuation
        :more="more"
        :loading="loadingMore"
        :error="moreError"
        @load="emit('load')"
      />
    </div>
  </div>
</template>
