<script setup lang="ts">
import type { usePlaylistBatchColumns } from '../../composables/usePlaylistBatchColumns';
import type { BatchResizeColumn } from '../../services/playlistBatchTable';
import TableColumnResize from '../TableColumnResize.vue';
defineProps<{
  model: ReturnType<typeof usePlaylistBatchColumns>;
  column: BatchResizeColumn;
  label: string;
  controls: string;
}>();
</script>
<template>
  <TableColumnResize v-bind="$props" class="batch-column-resizer" :data-batch-resize="column" />
</template>
