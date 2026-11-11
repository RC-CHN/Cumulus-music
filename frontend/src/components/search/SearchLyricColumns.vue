<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, useId, watch } from 'vue';
import { useTableColumns } from '../../composables/useTableColumns';
import {
  lyricColumnGrowth,
  lyricColumnShares,
  searchLyricColumns,
} from '../../services/searchLyricColumns';
import { searchTableLayouts, writeSearchColumnShares } from '../../services/searchTableColumns';
import TableColumnResize from '../TableColumnResize.vue';

const root = ref<HTMLElement>();
const bodyId = useId();
const definition = searchTableLayouts.lyrics;
const growth = shallowRef(lyricColumnGrowth());
const compactQuery =
  typeof matchMedia === 'function' ? matchMedia('(max-width: 950px)') : undefined;
const compact = ref(compactQuery?.matches ?? false);
const columns = useTableColumns(
  root,
  () => searchLyricColumns,
  () => compact.value,
  growth,
);
const style = computed(() =>
  columns.available.value
    ? { '--search-lyric-columns': columns.style.value.gridTemplateColumns }
    : undefined,
);
function updateCompact(event: MediaQueryListEvent) {
  compact.value = event.matches;
}
onMounted(() => compactQuery?.addEventListener('change', updateCompact));
onBeforeUnmount(() => compactQuery?.removeEventListener('change', updateCompact));

let saved = lyricColumnShares(growth.value);
watch([growth, columns.dragging], () => {
  if (!columns.available.value || columns.dragging.value !== undefined) return;
  const shares = lyricColumnShares(growth.value);
  if (!shares || shares.every((share, index) => Math.abs(share - (saved?.[index] ?? -1)) < 1e-8))
    return;
  saved = shares;
  writeSearchColumnShares('lyrics', shares);
});
</script>

<template>
  <div
    class="search-lyric-list"
    :data-lyric-columns="columns.available.value ? 'wide' : 'compact'"
    :style="style"
    ref="root"
  >
    <div class="track-columns track-header search-lyric-columns" role="row">
      <span
        v-for="(column, index) in definition.columns"
        :key="column.id"
        role="columnheader"
        :aria-label="column.label"
        :data-lyric-column="column.id"
      >
        {{ column.name }}
        <TableColumnResize
          v-if="columns.available.value && columns.resizable(index)"
          :model="columns"
          :column="index"
          :label="column.name"
          :controls="bodyId"
        />
      </span>
    </div>
    <slot :bodyId="bodyId" />
  </div>
</template>
