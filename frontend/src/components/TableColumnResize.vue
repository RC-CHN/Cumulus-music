<script setup lang="ts">
import type { useTableColumns } from '../composables/useTableColumns';
import '../styles/table-columns.css';
defineProps<{
  model: Pick<
    ReturnType<typeof useTableColumns>,
    'dragging' | 'minimum' | 'maximum' | 'value' | 'available' | 'start' | 'key' | 'reset'
  >;
  column: number;
  label: string;
  controls: string;
}>();
</script>
<template>
  <span
    class="table-column-resizer"
    :class="{ dragging: model.dragging.value === column }"
    role="separator"
    aria-orientation="vertical"
    :aria-label="`调整${label}列宽`"
    :aria-controls="controls"
    :aria-valuemin="model.minimum(column)"
    :aria-valuemax="Math.round(model.maximum(column))"
    :aria-valuenow="Math.round(model.value(column))"
    :aria-valuetext="`${Math.round(model.value(column))} 像素`"
    :aria-disabled="!model.available.value"
    :tabindex="model.available.value ? 0 : -1"
    :data-column-resize="column"
    :data-tooltip="`拖动调整${label}列宽；方向键微调，回车或双击重置`"
    @pointerdown="model.start($event, column)"
    @keydown="model.key($event, column)"
    @click.stop
    @dblclick.stop.prevent="model.reset"
  />
</template>
