<script setup lang="ts">
defineProps<{ text: string; progress: number; vertical: boolean }>();
</script>
<template>
  <span class="lyric-ink"
    ><span class="lyric-unplayed">{{ text }}</span
    ><span
      class="lyric-played"
      aria-hidden="true"
      :style="{
        clipPath: vertical
          ? `inset(0 0 ${100 - Math.max(0, Math.min(1, progress)) * 100}% 0)`
          : `inset(0 ${100 - Math.max(0, Math.min(1, progress)) * 100}% 0 0)`,
      }"
      >{{ text }}</span
    ></span
  >
</template>
