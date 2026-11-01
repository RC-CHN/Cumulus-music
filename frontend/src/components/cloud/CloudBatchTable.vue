<script setup lang="ts">
import { computed, useId } from 'vue';
import type { CloudTrack } from '../../services/cloud';
import { cloudBytes } from '../../services/cloud';
import { useBatchRows } from '../../composables/useBatchRows';
import { useTableColumns } from '../../composables/useTableColumns';
import TableColumnResize from '../TableColumnResize.vue';
const props = defineProps<{
  tracks: CloudTrack[];
  selected: Set<string>;
  disabled: boolean;
  currentId?: string;
}>();
const emit = defineEmits<{
  toggle: [index: number, range: boolean];
  select: [index: number];
  all: [force?: boolean];
}>();
const ids = computed(() => props.tracks.map((t) => t.track.id));
const { container, start, end, onScroll, active, toggle, key, focusBody, shortcut } = useBatchRows(
  ids,
  {
    selected: () => props.selected,
    disabled: () => props.disabled,
    toggle: (i, r) => emit('toggle', i, r),
    select: (i) => emit('select', i),
    all: (f) => emit('all', f),
  },
);
const definition = [
  { width: 45, grow: 0, resize: false },
  { width: 180, grow: 360, resize: true },
  { width: 115, grow: 230, resize: true },
  { width: 64, grow: 0, resize: false },
  { width: 90, grow: 45, resize: true },
  { width: 60, grow: 30, resize: false },
];
const columns = useTableColumns(
    container,
    () => definition,
    () => props.disabled,
  ),
  bodyId = useId();
const rows = computed(() => props.tracks.slice(start.value, end.value));
</script>
<template>
  <div
    class="cloud-batch-table"
    role="table"
    aria-label="全部云盘歌曲"
    :aria-rowcount="tracks.length + 1"
    @keydown="shortcut"
  >
    <div class="cloud-batch-inner">
      <div class="batch-columns batch-table-head" role="row" :style="columns.style.value">
        <span role="columnheader"
          ><input
            type="checkbox"
            aria-label="全选云盘歌曲"
            :checked="!!tracks.length && selected.size === tracks.length"
            :indeterminate="selected.size > 0 && selected.size < tracks.length"
            :disabled="disabled || !tracks.length"
            @change="emit('all')"
        /></span>
        <span role="columnheader" class="batch-title-heading"
          ><button class="batch-select-label" :disabled="disabled" @click="emit('all')">
            {{
              selected.size ? `已选 ${selected.size} 首歌曲` : `全选（共${tracks.length}首）`
            }}</button
          ><TableColumnResize :model="columns" :column="1" label="标题" :controls="bodyId"
        /></span>
        <span role="columnheader" class="batch-title-heading"
          >专辑<TableColumnResize :model="columns" :column="2" label="专辑" :controls="bodyId"
        /></span>
        <span role="columnheader">格式</span
        ><span role="columnheader" class="batch-title-heading"
          >上传时间<TableColumnResize
            :model="columns"
            :column="4"
            label="上传时间"
            :controls="bodyId" /></span
        ><span role="columnheader">大小</span>
      </div>
      <div
        ref="container"
        :id="bodyId"
        class="batch-table-scroll cloud-batch-scroll"
        tabindex="0"
        @focus.self="focusBody"
        @scroll.passive="onScroll"
        @keydown.self="key($event, active)"
      >
        <div :style="{ height: `${start * 64}px` }" aria-hidden="true" />
        <div
          v-for="(row, i) in rows"
          :key="row.track.id"
          class="batch-columns batch-track-row"
          :style="columns.style.value"
          :class="{
            alternate: (start + i) % 2 === 0,
            selected: selected.has(row.track.id),
            current: currentId === row.track.id,
          }"
          role="row"
          :aria-rowindex="start + i + 2"
          :aria-selected="selected.has(row.track.id)"
          :aria-disabled="disabled"
          :data-batch-index="start + i"
          :data-cloud-batch-id="row.track.id"
          tabindex="-1"
          @focus="active = start + i"
          @click="toggle($event, start + i)"
          @keydown="key($event, start + i)"
        >
          <span role="cell"
            ><input
              type="checkbox"
              tabindex="-1"
              :aria-label="`选择 ${row.track.name}`"
              :checked="selected.has(row.track.id)"
              :disabled="disabled"
              @click.stop="toggle($event, start + i)"
          /></span>
          <span role="cell" class="cloud-batch-title"
            ><strong class="batch-track-title" :title="row.track.name">{{ row.track.name }}</strong
            ><small :title="row.track.artists.join(' / ')">{{
              row.track.artists.join(' / ') || '未知歌手'
            }}</small></span
          >
          <span role="cell" class="ellipsis muted" :title="row.track.album">{{
            row.track.album || '未知专辑'
          }}</span
          ><span role="cell" class="muted">{{ row.format.toUpperCase() || '—' }}</span
          ><span role="cell" class="ellipsis muted">{{
            row.addedAt ? new Date(row.addedAt).toLocaleDateString('zh-CN') : '—'
          }}</span
          ><span role="cell" class="ellipsis muted" :title="cloudBytes(row.fileSize)">{{
            cloudBytes(row.fileSize)
          }}</span>
        </div>
        <div :style="{ height: `${Math.max(0, tracks.length - end) * 64}px` }" aria-hidden="true" />
      </div>
    </div>
  </div>
</template>
