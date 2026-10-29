<script setup lang="ts">
import type { DesktopLyricLine } from '../services/desktopLyrics';
import LyricChannel from './LyricChannel.vue';
defineProps<{ line: DesktopLyricLine; vertical: boolean; align: 'left' | 'center' | 'right' }>();
</script>
<template>
  <div class="desktop-lyric-row" :class="{ vertical }">
    <LyricChannel
      :text="line.text"
      :words="line.words"
      :active="line.active"
      :progress="line.progress"
      :vertical="vertical"
      :align="align"
    />
    <LyricChannel
      v-if="line.auxiliary"
      :text="line.auxiliary"
      :active="line.active"
      :progress="line.progress"
      :vertical="vertical"
      :align="align"
      auxiliary
    />
  </div>
</template>
