<script setup lang="ts">
import { computed } from 'vue';

const props = defineProps<{ modules: boolean[][]; label: string; width: number }>();
const size = computed(() => props.modules.length + 8);
const path = computed(() =>
  props.modules
    .flatMap((row, y) => row.flatMap((dark, x) => (dark ? [`M${x + 4} ${y + 4}h1v1h-1z`] : [])))
    .join(' '),
);
</script>

<template>
  <svg
    role="img"
    :aria-label="label"
    :viewBox="`0 0 ${size} ${size}`"
    :width="width"
    :height="width"
    shape-rendering="crispEdges"
  >
    <rect :width="size" :height="size" fill="#fff" />
    <path :d="path" fill="#000" />
  </svg>
</template>
