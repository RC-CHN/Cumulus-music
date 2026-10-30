<script setup lang="ts">
import { computed, useId } from 'vue';
import type { DownloadTask } from '../../services/downloads';
import { bytesLabel } from '../../services/downloads';
import { downloadDate, downloadTableColumns } from '../../services/downloadPage';
import type { BatchCategory, BatchOrder } from '../../services/downloadBatch';
import { useTableColumns } from '../../composables/useTableColumns';
import { useBatchRows } from '../../composables/useBatchRows';
import TableColumnResize from '../TableColumnResize.vue';
import DownloadTrackTitle from './DownloadTrackTitle.vue';
import Icon from '../Icon.vue';
const props = defineProps<{
  category: BatchCategory;
  tasks: DownloadTask[];
  selected: Set<string>;
  disabled: boolean;
  order?: BatchOrder;
}>();
const emit = defineEmits<{
  toggle: [index: number, range: boolean];
  select: [index: number];
  all: [force?: boolean];
  sort: [column: BatchOrder['column']];
}>();
const ids = computed(() => props.tasks.map((t) => t.id));
const { container, start, end, onScroll, active, toggle, key, focusBody, shortcut } = useBatchRows(
  ids,
  {
    selected: () => props.selected,
    disabled: () => props.disabled,
    toggle: (i, r) => emit('toggle', i, r),
    select: (i) => emit('select', i),
    all: (force) => emit('all', force),
  },
);
const cols = useTableColumns(
  container,
  () => downloadTableColumns(props.category),
  () => props.disabled,
);
const bodyId = useId(),
  unit = computed(() => (props.category === 'voice' ? '条声音' : '首歌曲'));
const headers = computed<{ column: BatchOrder['column']; label: string }[]>(() => [
  ...(props.category === 'song' ? [{ column: 'album' as const, label: '专辑' }] : []),
  { column: 'completed', label: '下载时间' },
  { column: 'size', label: '大小' },
]);
</script>
<template>
  <div
    class="playlist-batch-table download-batch-table"
    role="table"
    :aria-rowcount="tasks.length + 1"
    @keydown="shortcut"
  >
    <div class="batch-columns batch-table-head" role="row" :style="cols.style.value">
      <span role="columnheader"
        ><input
          type="checkbox"
          aria-label="全选下载"
          :checked="!!tasks.length && selected.size === tasks.length"
          :indeterminate="!!selected.size && selected.size < tasks.length"
          :disabled="disabled || !tasks.length"
          @change="emit('all')"
      /></span>
      <span role="columnheader" class="batch-title-heading"
        ><button
          class="batch-select-label"
          :disabled="disabled || !tasks.length"
          @click="emit('all')"
        >
          {{
            selected.size
              ? '已选 ' + selected.size + ' ' + unit
              : '全选（共' + tasks.length + unit + '）'
          }}
        </button>
        <TableColumnResize :model="cols" :column="1" label="标题" :controls="bodyId"
      /></span>
      <span
        v-for="(head, i) in headers"
        :key="head.column"
        role="columnheader"
        :aria-sort="
          order?.column === head.column ? (order.descending ? 'descending' : 'ascending') : 'none'
        "
      >
        <button
          class="download-sort-heading"
          :disabled="disabled"
          @click="emit('sort', head.column)"
        >
          {{ head.label
          }}<Icon v-if="order?.column === head.column" :name="order.descending ? 'down' : 'up'" />
        </button>
        <TableColumnResize
          v-if="cols.resizable(i + 2)"
          :model="cols"
          :column="i + 2"
          :label="head.label"
          :controls="bodyId"
      /></span>
    </div>
    <div
      ref="container"
      :id="bodyId"
      class="batch-table-scroll"
      tabindex="0"
      @scroll.passive="onScroll"
      @focus.self="focusBody"
      @keydown.self="key($event, active)"
    >
      <div :style="{ height: start * 64 + 'px' }" aria-hidden="true" />
      <div
        v-for="(task, i) in tasks.slice(start, end)"
        :key="task.id"
        class="batch-columns batch-track-row"
        :style="cols.style.value"
        :class="{ selected: selected.has(task.id) }"
        role="row"
        :aria-rowindex="start + i + 2"
        :aria-selected="selected.has(task.id)"
        :aria-disabled="disabled"
        :data-batch-index="start + i"
        :data-batch-task="task.id"
        tabindex="-1"
        @focus="active = start + i"
        @click="toggle($event, start + i)"
        @keydown="key($event, start + i)"
        @dblclick.prevent
      >
        <span role="cell"
          ><input
            type="checkbox"
            tabindex="-1"
            :aria-label="'选择 ' + task.track.name"
            :checked="selected.has(task.id)"
            :disabled="disabled"
            @click.stop="toggle($event, start + i)"
        /></span>
        <div role="cell">
          <DownloadTrackTitle
            :task="task"
            :active="false"
            :locked="disabled"
            :repairing="false"
            selection-only
          />
        </div>
        <span
          v-if="category === 'song'"
          role="cell"
          class="ellipsis"
          :data-tooltip="task.track.album"
          >{{ task.track.album || '—' }}</span
        >
        <span role="cell" class="download-time">{{ downloadDate(task.completedMs) }}</span
        ><span role="cell" class="download-size">{{ bytesLabel(task.total) }}</span>
      </div>
      <div :style="{ height: (tasks.length - end) * 64 + 'px' }" aria-hidden="true" />
    </div>
  </div>
</template>
